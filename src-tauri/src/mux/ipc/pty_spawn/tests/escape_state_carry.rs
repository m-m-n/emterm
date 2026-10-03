//! mux-strip-escape-state-carry task0001 (FR1-FR5, NFR1-NFR5): the write filter
//! carries the full end state of the bytes it WROTE after the strip (ground,
//! escape, designator wait, CSI entry / parameter) and writes at most one closure
//! per cut chosen from that state, so its output and state do not depend on where
//! PTY reads are split and a ring replay never completes a query or a reset the
//! client did not start (review finding `a879a02de382209f`).
//!
//! The scenario: the strip removes a construct together with its opening `ESC`
//! (an OSC 777 launch, OSC 9999 emterm-md, an agent-status report, a Kitty APC,
//! a SIXEL DCS, an answered CSI device query). When an `ESC` the strip WROTE
//! stands right before it, the written bytes still end in that `ESC`. Dropping
//! that state let the next call's `[6` start a CSI-less text, or a cut leave the
//! ring ending in a bare `ESC` that a replayed `[6n` / `c` completes into a
//! cursor-position report or a full reset.
//!
//! Since mux-strip-concat-query-closure (D1, FR4) the strip itself closes that
//! `ESC`: it writes one CSI_CLOSING (DEL) at the removed construct, so the written
//! bytes end in ground and a cut writes no closure after them. These tests pin
//! that merged behavior; the Escape closure (CAN) stays the cut's closure for a
//! written stream that genuinely ends in an `ESC` (R7).
//!
//! Oracle convention (IMPLEMENTATION.md): the TM-1 tests (R4, R6, R7, R10)
//! compare against term_core, never against expected bytes alone; the reference
//! is term_core fed the raw stream with a 47 / 1047 / 1049 `h` / `l` pair in
//! place of the cut, and the removed construct's own effects (the answer to
//! `ESC[6n`, Kitty responses and placements) are kept out of the comparison
//! (EC-9). R5, R8 and R9 pin the exact bytes. The end state of a byte stream is
//! read through `scrollback_filter::tests::client_written_state`.
//!
//! Test identifiers (IMPLEMENTATION.md, Regression test identifiers): R4 the
//! reproduction, R5 the Escape closure at a cut, R6 the raw-stream replay,
//! R7 the closure's effect in term_core, R8 the designator waits, R9 the overflow
//! flush, R10 the production reader, R11 the budget. R1 lives in
//! `post_strip_cut_csi`, R2 and R3 in `scrollback_filter::tests`.

use super::post_strip_cut_csi::{
    AGENT_STATUS, BUDGET, KITTY, LAUNCH, MD_LAUNCH, QUERY, SIXEL, all_targets,
    assert_client_equals_reference_except, own_answer_of,
};
use super::round3_write_path::{DIMS, run_visibility_restore_at, switch_pairs};
use super::round4_cut_csi::{
    emitted_through, osc_held_at_the_cap, text, view_after_a_cut, view_of,
};
use super::*;
use crate::mux::scrollback_filter::tests::client_written_state;
use crate::mux::scrollback_filter::{
    strip_pty_output_for_scrollback_write, strip_replayable_rich_content,
};

const ESC: &[u8] = &[0x1b];
const BEL: &[u8] = &[0x07];

/// `ESC` followed by `target`: the written `ESC` stands right before a construct
/// the strip removes together with its own opening `ESC`.
fn esc_then(target: &[u8]) -> Vec<u8> {
    [ESC, target].concat()
}

/// What the filter writes for `ESC` + a removed construct: the `ESC` and the
/// strip's closing at the removal (mux-strip-concat-query-closure D1).
fn esc_closed() -> Vec<u8> {
    [ESC, CSI_CLOSING].concat()
}

/// `ESC` followed by the Escape closure, the cut's closure for a written stream
/// that ends in an `ESC`.
fn esc_and_closure() -> Vec<u8> {
    [ESC, ESCAPE_CLOSING].concat()
}

/// A filter's output and state after being fed `pieces` as consecutive
/// cut-free calls (`cut_at_end`: the last call carries a cut at its end).
#[derive(Debug, PartialEq)]
struct Fed {
    emitted: Vec<u8>,
    pending: Vec<u8>,
    awaiting: bool,
    written: WrittenState,
}

fn feed_pieces(pieces: &[&[u8]], cut_at_end: bool) -> Fed {
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
    }
    Fed {
        emitted,
        pending: filter.pending().to_vec(),
        awaiting: filter.awaiting_designator(),
        written: filter.written_state(),
    }
}

/// At most one closure per cut (EC-8): after the stripped run `out` carries at
/// most one more byte, and that byte is one of DEL, the Escape closure and the
/// designator `ESC`.
#[track_caller]
fn assert_at_most_one_closure(out: &[u8], stripped_run: &[u8], ctx: &str) {
    assert!(
        out.starts_with(stripped_run),
        "{ctx}: the output {out:?} starts with the stripped run {stripped_run:?}"
    );
    let tail = &out[stripped_run.len()..];
    assert!(
        tail.len() <= 1,
        "{ctx}: at most one closure byte follows the run, got {tail:?}"
    );
    if let Some(byte) = tail.first() {
        assert!(
            [CSI_CLOSING[0], ESCAPE_CLOSING[0], 0x1b].contains(byte),
            "{ctx}: the closure byte {byte:#04x} is DEL, the Escape closure or the designator ESC"
        );
    }
}

// ── R4 (AC-7, TS-1): the reproduction ─────────────────────────────────────

/// AC-7 (FR3, FR5, NFR3, TM-1, TS-1, R4): for each held target T and for the CSI
/// query, call 1 `ESC` + T without a cut writes `ESC` + DEL (the strip closes the
/// written `ESC` at the removal, mux-strip-concat-query-closure D1) and leaves
/// ground, and call 2 `[6` with a trailing cut writes `[6` as text and no
/// closure. term_core replaying the ring and then a later `n` gives no response
/// (no cursor-position report) and displays `[6n`.
#[test]
fn escape_carry_the_reproduction_closes_the_csi_and_replays_no_query() {
    for (name, target) in all_targets() {
        let mut filter = ScrollbackWriteFilter::new();
        let mut ring = filter.feed(&esc_then(target), DIMS).1;
        assert_eq!(
            ring,
            esc_closed(),
            "{name}: call 1 writes the ESC and the closing"
        );
        assert!(filter.pending().is_empty(), "{name}");
        assert_eq!(
            filter.written_state(),
            WrittenState::Ground,
            "{name}: the closing leaves the written stream in ground"
        );

        let cut = filter.feed_with_cuts(b"[6", DIMS, &[2]);
        ring.extend_from_slice(&cut.bytes);
        assert_eq!(
            ring,
            [&esc_closed()[..], b"[6"].concat(),
            "{name}: call 2 writes `[6` as text and no closure"
        );
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
        assert!(!filter.awaiting_designator(), "{name}");

        // The same bytes as one call.
        let one_call = emitted_through(
            &[&esc_then(target)[..], b"[6"].concat(),
            &[1 + target.len() + 2],
            &[],
        );
        assert_eq!(one_call, ring, "{name}: one call writes the same bytes");

        // A later `n`: fed through the filter, replayed through term_core.
        ring.extend_from_slice(&filter.feed(b"n", DIMS).1);
        let view = view_of(&ring);
        assert!(
            view.responses.is_empty(),
            "{name}: the replay answers no cursor-position report: {:?}",
            view.responses
        );
        assert_eq!(view.rows[0], "[6n", "{name}: `[6n` is displayed as text");
    }
}

// ── R5 (AC-3, AC-4 first rows, TS-3, EC-1..EC-3, EC-8): the Escape closure ──

/// AC-3 (FR3, FR4, NFR3, TM-1, TS-3, R5, EC-1, EC-2, EC-3, EC-8): after `ESC` +
/// each removed construct the written bytes are `ESC` + DEL (the strip's closing,
/// mux-strip-concat-query-closure D1) and end in ground, so a cut writes no
/// closure after them, whichever way the cut arrives; a fallback closing writes
/// nothing; afterwards the state is Ground and no designator is awaited.
#[test]
fn escape_carry_a_cut_after_a_written_escape_writes_the_escape_closure() {
    let closing = esc_closed();
    for (name, target) in all_targets() {
        let fed = esc_then(target);
        let stripped = strip_pty_output_for_scrollback_write(&fed);
        assert_eq!(
            stripped, closing,
            "{name}: the strip writes the ESC and the closing"
        );

        // A cut at the end of the call.
        let mut filter = ScrollbackWriteFilter::new();
        let out = filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes;
        assert_eq!(out, closing, "{name}: cut at the end of the call");
        assert_at_most_one_closure(&out, &stripped, name);
        assert!(filter.pending().is_empty(), "{name}");
        assert!(!filter.awaiting_designator(), "{name}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");

        // Three cuts at that same position: one closure.
        let mut filter = ScrollbackWriteFilter::new();
        let out = filter
            .feed_with_cuts(&fed, DIMS, &[fed.len(), fed.len(), fed.len()])
            .bytes;
        assert_eq!(out, closing, "{name}: three cuts at the same position");
        assert_at_most_one_closure(&out, &stripped, name);

        // The construct fed in an earlier cut-free call, then the reader's
        // fallback closing (an empty range with a cut at 0); a second one
        // writes nothing.
        let mut filter = ScrollbackWriteFilter::new();
        let mut got = filter.feed(&fed, DIMS).1;
        assert_eq!(
            got, closing,
            "{name}: the cut-free call writes the ESC and the closing"
        );
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
        got.extend_from_slice(&filter.feed_with_cuts(b"", DIMS, &[0]).bytes);
        assert_eq!(got, closing, "{name}: fallback closing");
        assert_at_most_one_closure(&got, &stripped, name);
        assert!(
            filter.feed_with_cuts(b"", DIMS, &[0]).bytes.is_empty(),
            "{name}: a second fallback closing writes nothing"
        );
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
        assert!(!filter.awaiting_designator(), "{name}");

        // The cut ends the call, the bytes after it start from ground.
        let fed_after = [&fed[..], b"abc"].concat();
        let mut filter = ScrollbackWriteFilter::new();
        let out = filter.feed_with_cuts(&fed_after, DIMS, &[fed.len()]).bytes;
        assert_eq!(
            out,
            [&closing[..], b"abc"].concat(),
            "{name}: the cut inside the call, then plain text"
        );

        // EC-1: a CSI interrupted by the written ESC. The written ESC aborts
        // the CSI and the strip closes it at the removal, so the cut writes no
        // closure.
        let fed = [&b"\x1b[6"[..], &esc_then(target)].concat();
        let stripped = strip_pty_output_for_scrollback_write(&fed);
        assert_eq!(stripped, b"\x1b[6\x1b\x7f", "{name}: EC-1 strip");
        let mut filter = ScrollbackWriteFilter::new();
        let out = filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes;
        assert_eq!(
            out, stripped,
            "{name}: EC-1 writes `ESC[6 ESC` and the strip's closing"
        );
        assert!(
            !out.contains(&ESCAPE_CLOSING[0]),
            "{name}: EC-1 writes no Escape closure"
        );
        assert_at_most_one_closure(&out, &stripped, name);

        // EC-2: a construct held after the written ESC and dropped by the cut.
        let fed = [&esc_then(target)[..], b"\x1b]0;ti"].concat();
        let mut filter = ScrollbackWriteFilter::new();
        let out = filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes;
        assert_eq!(
            out, closing,
            "{name}: EC-2 writes `ESC` and the closing, not the held OSC"
        );
        assert!(filter.pending().is_empty(), "{name}: EC-2");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");

        // EC-3: a second call of `ESC` + the construct writes `ESC` + DEL
        // again and leaves ground.
        let mut filter = ScrollbackWriteFilter::new();
        let first = filter.feed(&esc_then(target), DIMS).1;
        assert_eq!(first, closing, "{name}: EC-3 first call");
        let second = filter.feed(&esc_then(target), DIMS).1;
        assert_eq!(second, closing, "{name}: EC-3 second call");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
        assert!(filter.pending().is_empty(), "{name}: EC-3");
        // The next removed construct is still removed.
        let third = filter.feed(target, DIMS).1;
        assert!(
            third.is_empty(),
            "{name}: EC-3 a following construct is removed in ground, without a closing"
        );
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
    }
}

// ── R6 (AC-7, TS-3, EC-9): the closure replays like the raw stream ────────

/// Continuations that a client still inside the written ESC would complete into
/// a query (`[6n`, `[c`), a reset (`c`) or an unknown escape, and that a client
/// in ground displays as text.
const COMPLETING_CONTINUATIONS: &[&[u8]] = &[b"[6n", b"n", b"c"];

/// AC-7 (FR3, NFR3, TM-1, TS-3, R6, EC-9): at a cut right after `ESC` + each
/// removed construct (written as `ESC` + the strip's closing) - the cut ending the call, the cut and the continuation in
/// one call, and the reader's fallback closing - term_core fed the emitted ring
/// and then each of `[6n`, `n` and `c` shows the same rows and cursor and gives
/// the same responses after the cut as term_core fed the raw stream with the
/// removed 47 / 1047 / 1049 `h` / `l` pair inline. No cursor-position report,
/// device-attributes answer or reset appears.
#[test]
fn escape_carry_the_escape_closure_replays_like_the_raw_stream() {
    let start = std::time::Instant::now();
    for (form, construct) in all_targets() {
        let prefix = esc_then(construct);
        for (enter, leave) in switch_pairs() {
            let pair = [enter, leave].concat();
            let before_raw = [&prefix[..], &pair[..]].concat();
            for continuation in COMPLETING_CONTINUATIONS.iter().copied() {
                let ctx = format!(
                    "{form}, switch {}, continuation {:?}",
                    text(enter),
                    text(continuation)
                );
                let reference = view_after_a_cut(&before_raw, continuation);
                assert!(
                    reference.responses.is_empty(),
                    "{ctx}: the continuation is plain text in the raw stream"
                );

                // The cut ends the call; the continuation arrives in a later
                // one.
                let mut filter = ScrollbackWriteFilter::new();
                let closed = filter.feed_with_cuts(&prefix, DIMS, &[prefix.len()]).bytes;
                assert_eq!(closed, esc_closed(), "{ctx}");
                let later = filter.feed(continuation, DIMS).1;
                let view = view_after_a_cut(&closed, &later);
                assert!(
                    view.responses.is_empty(),
                    "{ctx}: no CPR, DA or RIS answer: {:?}",
                    view.responses
                );
                assert_eq!(
                    view, reference,
                    "{ctx}: cut at the end of the call, continuation in a later call"
                );

                // The cut and the continuation in one call.
                let mut filter = ScrollbackWriteFilter::new();
                let fed = [&prefix[..], continuation].concat();
                let whole = filter.feed_with_cuts(&fed, DIMS, &[prefix.len()]).bytes;
                assert!(
                    whole.starts_with(&closed),
                    "{ctx}: same bytes before the cut"
                );
                assert_eq!(
                    view_after_a_cut(&closed, &whole[closed.len()..]),
                    reference,
                    "{ctx}: cut and continuation in the same call"
                );

                // The construct in an earlier call, the reader's fallback
                // closing, then the continuation.
                let mut filter = ScrollbackWriteFilter::new();
                let mut before = filter.feed(&prefix, DIMS).1;
                before.extend_from_slice(&filter.feed_with_cuts(b"", DIMS, &[0]).bytes);
                assert_eq!(before, esc_closed(), "{ctx}: fallback closing");
                let later = filter.feed(continuation, DIMS).1;
                assert_eq!(
                    view_after_a_cut(&before, &later),
                    reference,
                    "{ctx}: fallback closing"
                );
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── R7 (AC-6, TS-3, FR4): the closure has no effect in term_core ──────────

/// Streams that end in a written `ESC`.
const ESCAPE_ENDING_STREAMS: &[&[u8]] = &[b"\x1b", b"abc\x1b", b"\x1b[6\x1b", b"\x1b[1m\x1b"];

/// Probe text after the closure: continuations that complete a query, a reset
/// or a cursor move from the written `ESC`, and letters DEC line drawing would
/// change.
const CLOSURE_PROBES: &[&[u8]] = &[
    b"[6n", b"n", b"c", b"[c", b"lqkxmj", b"abc", b"qqqq", b"[H", b"7", b"M", b"(0q",
];

/// AC-6 (FR4, TM-1, TS-3, R7): the Escape closure is CAN (0x18), a constant next
/// to and distinct from `CSI_CLOSING`. For streams ending in a written ESC:
/// (a) the stream followed by the closure ends in Ground per the oracle;
/// (b) its rows, cursor and responses equal those of the stream, and probe text
///     after it displays exactly as after the stream closed to ground without
///     its trailing ESC (an open CSI the ESC aborted is cancelled by DEL), with
///     no response to a following `[6n`, `n`, `c` or `[c`;
/// (c)/(d) the closure is neither ESC nor `\`; both strips keep `ESC` + the
///     closure as written and still remove a strip target right after it, and
///     inserting `ESC` + the closure between a Kitty APC or SIXEL DCS introducer
///     and the ST that ends it leaves the snapshot strip's output unchanged (no
///     new ST).
#[test]
fn escape_carry_the_escape_closure_has_no_effect_in_term_core() {
    assert_eq!(ESCAPE_CLOSING, &[0x18][..], "the Escape closure is CAN");
    assert_ne!(ESCAPE_CLOSING, CSI_CLOSING, "distinct from DEL");
    assert_eq!(ESCAPE_CLOSING.len(), 1, "one byte");

    for stream in ESCAPE_ENDING_STREAMS.iter().copied() {
        let label = text(stream);
        let closed = [stream, ESCAPE_CLOSING].concat();
        // (a)
        assert_eq!(
            client_written_state(stream),
            WrittenState::Escape,
            "{label}: the stream ends in Escape"
        );
        assert_eq!(
            client_written_state(&closed),
            WrittenState::Ground,
            "{label}: the closure returns term_core to ground"
        );
        // (b)
        assert_eq!(
            view_of(&closed),
            view_of(stream),
            "{label}: the closure changes no row, cursor or response"
        );
        let before_esc = &stream[..stream.len() - 1];
        let reference: Vec<u8> = match client_written_state(before_esc) {
            WrittenState::Csi(_) => [before_esc, CSI_CLOSING].concat(),
            _ => before_esc.to_vec(),
        };
        assert_eq!(
            client_written_state(&reference),
            WrittenState::Ground,
            "{label}"
        );
        for probe in CLOSURE_PROBES.iter().copied() {
            let got = view_after_a_cut(&closed, probe);
            assert_eq!(
                got,
                view_after_a_cut(&reference, probe),
                "{label}: probe {:?} after the closure displays as after the stream \
                 without its trailing ESC",
                text(probe)
            );
            assert!(
                got.responses.is_empty(),
                "{label}: probe {:?} gets no response: {:?}",
                text(probe),
                got.responses
            );
        }
    }

    // (c): the closure is neither ESC nor `\`, so it forms no ST with the ESC.
    assert_ne!(ESCAPE_CLOSING[0], 0x1b);
    assert_ne!(ESCAPE_CLOSING[0], b'\\');

    // (c)/(d): both strips keep `ESC` + the closure and still remove a strip
    // target right after it.
    let kept = esc_and_closure();
    assert_eq!(strip_pty_output_for_scrollback_write(&kept), kept);
    assert_eq!(strip_replayable_rich_content(&kept), kept);
    for (name, target) in all_targets() {
        let input = [&kept[..], target, b"x"].concat();
        let want = [&kept[..], b"x"].concat();
        assert_eq!(
            strip_pty_output_for_scrollback_write(&input),
            want,
            "{name}: write-path strip"
        );
        assert_eq!(
            strip_replayable_rich_content(&input),
            want,
            "{name}: snapshot strip"
        );
    }

    // (d): `ESC` + the closure inserted anywhere between the introducer of a
    // Kitty APC or a SIXEL DCS and the ST that ends it adds no ST: the
    // snapshot strip removes the same span.
    for (name, introducer_len, construct) in [("kitty apc", 3, KITTY), ("sixel dcs", 3, SIXEL)] {
        let st_start = construct.len() - 2;
        let plain = [&b"head"[..], construct, b"tail"].concat();
        let want = strip_replayable_rich_content(&plain);
        assert_eq!(want, b"headtail", "{name}: the plain construct is removed");
        for at in introducer_len..=st_start {
            let inserted = [&construct[..at], &kept[..], &construct[at..]].concat();
            let input = [&b"head"[..], &inserted[..], b"tail"].concat();
            assert_eq!(
                strip_replayable_rich_content(&input),
                want,
                "{name}: ESC + closure inserted at {at}"
            );
        }
    }
}

// ── R8 (AC-4, TS-4, EC-4, EC-5): splice-made designator waits ─────────────

/// AC-4 (FR2, FR3, TS-4, R8, EC-4, EC-5): for `ESC` + a removed construct + `(`
/// (and `)`) the strip closes the written `ESC` at the removal
/// (mux-strip-concat-query-closure D1), so no designator wait is spliced:
/// - the written bytes are `ESC` + DEL + the brace as text, the state is Ground
///   and nothing is awaited; a cut at the end writes no closure, neither the
///   designator ESC nor DEL nor the Escape closure after them;
/// - a following call that starts with a removed construct has it removed (the
///   same bytes as one call), and a fallback closing writes nothing.
/// EC-5: for `ESC` + a construct + `( ESC (`, the written `ESC (` is a designation
/// start of its own: without a cut the awaiting flag is set and the state is
/// Designator; a fallback closing and a cut at the end write the designator ESC
/// once, and a following call's first byte is copied verbatim (the same bytes as
/// one call).
#[test]
fn escape_carry_a_splice_made_designator_wait_closes_with_the_designator_esc() {
    for (name, target) in all_targets() {
        for brace in [b'(', b')'] {
            let ctx = format!("{name}, brace {:?}", brace as char);
            let designation = [0x1b, brace];
            let fed = [&esc_then(target)[..], &[brace]].concat();
            let written = [&esc_closed()[..], &[brace]].concat();

            // A cut at the end: the written bytes and no closure.
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes;
            assert_eq!(out, written, "{ctx}: cut at the end writes no closure");
            assert!(
                !out[written.len() - 1..].contains(&ESCAPE_CLOSING[0]),
                "{ctx}: no Escape closure"
            );
            assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
            assert!(!filter.awaiting_designator(), "{ctx}");

            // No cut: the state is Ground, nothing is awaited.
            let mut filter = ScrollbackWriteFilter::new();
            let first = filter.feed(&fed, DIMS).1;
            assert_eq!(first, written, "{ctx}: the written bytes");
            assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
            assert!(
                !filter.awaiting_designator(),
                "{ctx}: the boundary scan does not await a designator"
            );
            assert!(filter.pending().is_empty(), "{ctx}");
            // A following call starting with a removed construct: removed as in
            // one call.
            let second_fed = [target, b"abc"].concat();
            let second = filter.feed(&second_fed, DIMS).1;
            assert_eq!(
                second, b"abc",
                "{ctx}: the construct at the start is removed"
            );
            let one_call = feed_pieces(&[&[&fed[..], &second_fed[..]].concat()], false);
            assert_eq!(
                [&first[..], &second[..]].concat(),
                one_call.emitted,
                "{ctx}: the same bytes as one call"
            );
            // The fallback closing writes nothing (a fresh filter in the same
            // state).
            let mut filter = ScrollbackWriteFilter::new();
            filter.feed(&fed, DIMS);
            let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
            assert!(
                closing.is_empty(),
                "{ctx}: the fallback closing: {closing:?}"
            );
            assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");

            // EC-5: `ESC` + construct + brace + `ESC` + brace.
            let splice = [&fed[..], &designation[..]].concat();
            let written = [&written[..], &designation[..]].concat();
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed(&splice, DIMS).1;
            assert_eq!(out, written, "{ctx}: EC-5 written bytes");
            assert!(
                filter.awaiting_designator(),
                "{ctx}: EC-5 the boundary scan awaits a designator"
            );
            assert_eq!(
                filter.written_state(),
                WrittenState::Designator,
                "{ctx}: EC-5 the written `ESC (` awaits its designator"
            );
            // A later fallback closing writes the designator ESC once.
            let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
            assert_eq!(closing, ESC, "{ctx}: EC-5 fallback closing");
            assert!(
                filter.feed_with_cuts(b"", DIMS, &[0]).bytes.is_empty(),
                "{ctx}: EC-5 one closing"
            );
            assert!(!filter.awaiting_designator(), "{ctx}: EC-5");
            // A cut at the end of the call writes the designator ESC.
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed_with_cuts(&splice, DIMS, &[splice.len()]).bytes;
            assert_eq!(
                out,
                [&written[..], ESC].concat(),
                "{ctx}: EC-5 cut at the end"
            );
            assert!(!filter.awaiting_designator(), "{ctx}: EC-5");
            // A following call's first byte is copied verbatim.
            let mut filter = ScrollbackWriteFilter::new();
            let mut got = filter.feed(&splice, DIMS).1;
            let next = [LAUNCH, b"x"].concat();
            got.extend_from_slice(&filter.feed(&next, DIMS).1);
            let one_call = feed_pieces(&[&[&splice[..], &next[..]].concat()], false);
            assert_eq!(
                got, one_call.emitted,
                "{ctx}: EC-5 the first byte after the wait is verbatim, as in one call"
            );
            assert!(
                got.ends_with(&next),
                "{ctx}: EC-5 the construct after the wait is kept whole: {:?}",
                text(&got)
            );
        }
    }
}

// ── R9 (AC-5, TS-5): the overflow flush ───────────────────────────────────

/// AC-5 (FR1, FR3, TS-5, R9): an OSC held at the 512 KiB cap is completed past
/// the cap by a call whose run ends in `ESC` + a complete removed construct (for
/// each removed construct). The stripped run ends in `ESC` + DEL (the strip's
/// closing, mux-strip-concat-query-closure D1), so with a cut at the end of that
/// call the output is the stripped run alone and the state is Ground; without a
/// cut the output is the stripped run, the state is Ground, and a later fallback
/// closing writes nothing.
#[test]
fn escape_carry_an_overflow_flush_ending_in_a_written_escape_carries_or_closes_it() {
    let held = osc_held_at_the_cap();
    for (name, target) in all_targets() {
        // The continuation of the held OSC: more body, its BEL, text and a
        // written ESC followed by the construct.
        let tail: Vec<u8> = [&b"pppppppppp"[..], BEL, b"abc", &esc_then(target)[..]].concat();
        let stripped = strip_pty_output_for_scrollback_write(&[&held[..], &tail[..]].concat());
        assert!(
            stripped.ends_with(b"abc\x1b\x7f"),
            "{name}: the run is stripped"
        );

        // A cut at the end of the flushing call.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty(), "{name}");
        let outcome = filter.feed_with_cuts(&tail, DIMS, &[tail.len()]);
        assert!(outcome.carried.is_none(), "{name}");
        assert!(
            outcome.bytes == stripped,
            "{name}: the stripped run and no closure"
        );
        assert!(filter.pending().is_empty(), "{name}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
        assert!(!filter.awaiting_designator(), "{name}");

        // A cut followed by text in the same call.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty(), "{name}");
        let fed = [&tail[..], b"abc"].concat();
        let outcome = filter.feed_with_cuts(&fed, DIMS, &[tail.len()]);
        assert!(
            outcome.bytes == [&stripped[..], b"abc"].concat(),
            "{name}: cut, then text in the same call"
        );

        // No cut: the flush carries Ground; a later fallback closing writes
        // nothing.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty(), "{name}");
        let outcome = filter.feed_with_cuts(&tail, DIMS, &[]);
        assert!(
            outcome.bytes == stripped,
            "{name}: the flush emits the stripped run"
        );
        assert_eq!(
            filter.written_state(),
            WrittenState::Ground,
            "{name}: the flush carries the post-strip state"
        );
        assert!(!filter.awaiting_designator(), "{name}");
        let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
        assert!(closing.is_empty(), "{name}: the fallback closing");
        assert!(
            filter.feed_with_cuts(b"", DIMS, &[0]).bytes.is_empty(),
            "{name}: once"
        );
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
    }
}

// ── R10 (AC-7, TS-7): the production reader ───────────────────────────────

/// AC-7 (FR3, NFR3, TM-1, TS-7, R10): through the production reader and the
/// production visibility restore of a main-screen pane whose ring has not
/// wrapped, with `ESC` + each removed construct and a removed 47 / 1047 / 1049
/// `h` / `l` pair in one read and `[6n` or `c` in a later read:
/// - the restore covering the switch read;
/// - the restore between the switch read and the later read.
/// The client's responses, screen and cursor equal the raw-stream reference's
/// (for the CSI-query form, apart from the reference's own answer to the removed
/// query, which the client gives only when the restore covers the read holding
/// the query and the reader re-delivers it live): the live parser's `ESC` was
/// completed by the removed construct, so the later `[6n` / `c` is plain text
/// there, and the ring's closure keeps it plain text on replay (no cursor-position
/// report, no device-attributes answer, no reset).
#[test]
fn escape_carry_reader_restore_matches_the_raw_stream_reference() {
    for (form, construct) in all_targets() {
        for (enter, leave) in switch_pairs() {
            let pair = [enter, leave].concat();
            let prefix = esc_then(construct);
            let own = own_answer_of(&prefix);
            assert_eq!(
                own.is_empty(),
                form != "csi query",
                "{form}: only the query form has an answer of its own"
            );
            let together = [&prefix[..], &pair[..]].concat();
            for continuation in [&b"[6n"[..], b"c"] {
                let label = format!("{form}, {}, {}", text(enter), text(continuation));

                // One read; the snapshot covers the switch read.
                let chunks = vec![together.clone(), continuation.to_vec()];
                let run = run_visibility_restore_at(&chunks, 0);
                assert_client_equals_reference_except(
                    &run.received,
                    &chunks,
                    &own,
                    &own,
                    &format!("(a) {label}"),
                );

                // The snapshot between the switch read and the later read.
                let chunks = vec![together.clone(), b"\r".to_vec(), continuation.to_vec()];
                let run = run_visibility_restore_at(&chunks, 1);
                assert_client_equals_reference_except(
                    &run.received,
                    &chunks,
                    &own,
                    &[],
                    &format!("(b) {label}"),
                );
            }
        }
    }
}

// ── R11 (AC-8, TS-8): adversarial input ───────────────────────────────────

/// `reps` copies of `ESC` followed by `target`.
fn alternating(target: &[u8], reps: usize) -> Vec<u8> {
    let unit = esc_then(target);
    std::iter::repeat_n(unit, reps).flatten().collect()
}

/// AC-8 (NFR2, NFR4, TM-2, TS-8, R11): long inputs alternating `ESC` + a removed
/// construct (each written as `ESC` + the strip's closing, which leaves ground,
/// so a trailing cut writes no closure) finish within 10 seconds, never panic,
/// and write the same bytes for
/// every feeding of the same input, with and without a trailing cut: below the
/// 512 KiB cap in one call, in two calls split at a unit boundary and inside a
/// unit, and byte at a time (on a shorter input of the same shape); above the cap
/// in one call and in two calls split at a unit boundary. The CSI-query form runs
/// in one call only (a query split across calls is written as it arrives, EC-7).
/// The one-pass and budget tests stay as they are; this adds no loop over the fed
/// bytes to the write filter.
#[test]
fn escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget() {
    let start = std::time::Instant::now();
    for (name, target) in [
        ("osc 777 launch", LAUNCH),
        ("osc 9999 emterm-md", MD_LAUNCH),
        ("agent-status", AGENT_STATUS),
        ("kitty apc", KITTY),
        ("sixel dcs", SIXEL),
    ] {
        let unit_len = 1 + target.len();

        // Below the cap, one call and two calls.
        let reps = 8_000;
        let input = alternating(target, reps);
        assert!(input.len() < SCROLLBACK_FILTER_PENDING_CAP, "{name}");
        for cut_at_end in [false, true] {
            let whole = feed_pieces(&[&input], cut_at_end);
            assert_eq!(
                whole.emitted,
                esc_closed().repeat(reps),
                "{name}: one call, cut at end {cut_at_end}"
            );
            assert!(whole.pending.is_empty(), "{name}");
            assert!(!whole.awaiting, "{name}");
            assert_eq!(
                whole.written,
                WrittenState::Ground,
                "{name}: cut at end {cut_at_end}"
            );
            for at in [
                1,
                unit_len,
                unit_len * 3,
                // Inside a unit: after its ESC, inside the construct, one byte
                // short of its end.
                unit_len * 7 + 1,
                unit_len * 7 + 1 + target.len() / 2,
                unit_len * 7 + unit_len - 1,
                input.len() / 2,
                input.len() - unit_len,
                input.len() - 1,
            ] {
                let (a, b) = input.split_at(at);
                assert_eq!(
                    feed_pieces(&[a, b], cut_at_end),
                    whole,
                    "{name}: split at {at}, cut at end {cut_at_end}"
                );
            }
        }

        // Byte at a time, on a shorter input of the same shape.
        let short = alternating(target, 1_500);
        for cut_at_end in [false, true] {
            let whole = feed_pieces(&[&short], cut_at_end);
            let bytes: Vec<&[u8]> = short.chunks(1).collect();
            assert_eq!(
                feed_pieces(&bytes, cut_at_end),
                whole,
                "{name}: byte by byte, cut at end {cut_at_end}"
            );
            assert_eq!(
                whole.emitted,
                esc_closed().repeat(1_500),
                "{name}: byte by byte writes the same bytes"
            );
        }

        // Above the cap, one call and two calls split at a unit boundary.
        let reps = 40_000usize.max(SCROLLBACK_FILTER_PENDING_CAP / unit_len + 10);
        let input = alternating(target, reps);
        assert!(input.len() > SCROLLBACK_FILTER_PENDING_CAP, "{name}");
        for cut_at_end in [false, true] {
            let whole = feed_pieces(&[&input], cut_at_end);
            assert_eq!(
                whole.emitted,
                esc_closed().repeat(reps),
                "{name}: overflow, one call, cut at end {cut_at_end}"
            );
            let (a, b) = input.split_at(unit_len * (reps / 2));
            assert_eq!(
                feed_pieces(&[a, b], cut_at_end),
                whole,
                "{name}: overflow, two calls, cut at end {cut_at_end}"
            );
        }
    }

    // A CSI device query is removed when it arrives whole in a call.
    let reps = 8_000;
    let input = alternating(QUERY, reps);
    for cut_at_end in [false, true] {
        let whole = feed_pieces(&[&input], cut_at_end);
        let expected = esc_closed().repeat(reps);
        assert_eq!(
            whole.emitted, expected,
            "query: one call, cut at end {cut_at_end}"
        );
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}
