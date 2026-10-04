//! mux-suppressed-output-round4-fixes task0002 (FR3, review finding
//! `48caec6f5b0b5810`): the write filter's closing write for a client that is
//! waiting for a charset designator at a cut.
//!
//! When the filter closes at a cut (a removed 47 / 1047 / 1049 switch, or the
//! reader's fallback closing) while the run it emitted ends in `ESC (` /
//! `ESC )`, the client's parser consumed the switch's ESC as the designator.
//! The filter then writes exactly one ESC after the bytes it emitted, so the
//! ring replays that designator and the bytes after the cut start from
//! ground. Nothing is written when no designator is awaited.
//!
//! Filter-level tests have one test per path so a regression names the path
//! that broke. Reader-level tests (the registry test and the EC-5 cases) use
//! the visibility-restore harness widened from `round3_write_path`; they
//! compare responses and the parse of the bytes after the closing with the
//! raw-stream reference, not the screen, because a switch sitting in the
//! designator slot is FR5's display question (SPEC AC-2, EC-5).

use super::round3_write_path::{
    DIMS, client_view, new_core, reference_view, run_reader_without_owner,
    run_visibility_restore_at, switch_pairs,
};
use super::*;

const ESC: u8 = 0x1b;

/// A complete OSC 11 color query: answered by the oracle responder exactly
/// when the client parses it from ground.
const COLOR_QUERY: &[u8] = b"\x1b]11;?\x07";

/// The start of the oracle responder's answer to [`COLOR_QUERY`].
const COLOR_ANSWER: &[u8] = b"\x1b]11;rgb:";

/// A complete viewer launch: a strip target when it starts from ground.
const LAUNCH: &[u8] = b"\x1b]777;emterm;markdown;begin;id=x\x07";

const BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

/// The number of color-query answers a client produces when it replays
/// `emitted` and is then fed one complete color query. One answer means the
/// client is at ground after `emitted`; none means it was still waiting for a
/// designator (the query's ESC was consumed as the designator).
fn answers_to_a_query_after(emitted: &[u8]) -> usize {
    let mut core = new_core();
    core.process_pty_data_fully(emitted);
    let _discarded = core.take_response();
    core.process_pty_data_fully(COLOR_QUERY);
    r2_count(&core.take_response(), COLOR_ANSWER)
}

/// `emitted` leaves the client at ground: the next byte starts a sequence.
fn assert_at_ground_after(emitted: &[u8], ctx: &str) {
    assert_eq!(
        answers_to_a_query_after(emitted),
        1,
        "{ctx}: a color query after the emitted bytes must be answered, so the client is at ground"
    );
}

/// Compare two long byte strings without printing them whole: on a mismatch
/// the report names the lengths, the first differing offset and a short
/// window around it.
#[track_caller]
fn assert_same_bytes(got: &[u8], expected: &[u8], ctx: &str) {
    if got == expected {
        return;
    }
    let at = got
        .iter()
        .zip(expected)
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(expected.len()));
    let window = |bytes: &[u8]| bytes[at.saturating_sub(4)..(at + 8).min(bytes.len())].to_vec();
    panic!(
        "{ctx}: bytes differ (got {} bytes, expected {}); first difference at {at}: got ..{:?}, expected ..{:?}",
        got.len(),
        expected.len(),
        window(got),
        window(expected)
    );
}

fn cut_feed(filter: &mut ScrollbackWriteFilter, fed: &[u8], cuts: &[usize]) -> FeedOutcome {
    filter.feed_with_cuts(fed, DIMS, cuts)
}

/// `ESC <brace>`: the two designator introducers.
fn braces() -> [u8; 2] {
    [b'(', b')']
}

// ── AC-1 (FR3, TM-1, registry): the task example ─────────────────────────

/// AC-1 (FR3, TM-1, TS-7, registry): `X ESC (`, a removed 47 / 1047 / 1049
/// `h` / `l` pair, `ESC (`, `ESC ]11;?`, BEL, through the production
/// visibility restore (snapshot, replacement and following chunks) of a
/// main-screen pane whose ring has not wrapped. The BEL arrives in the same
/// read and, in the other layouts, in a later read that reaches the client
/// live.
///
/// The client's first ESC after `ESC (` is the designator, so the stream
/// holds no OSC 11 query. The pre-fix ring holds `X ESC ( ESC ( ESC ]11;?`:
/// the switch's ESC is gone, so the snapshot's replay takes the second `(` as
/// the designator, parses `ESC ]11;?` as an OPEN OSC, and a BEL that reaches
/// the client afterwards completes it into a color-query answer the raw
/// stream never produced. After the fix the ring holds the designator ESC,
/// the client's responses equal the raw-stream reference's, and a later
/// complete query is answered the same number of times in both.
///
/// round4 FR5 (task0005): the ESC of the `enter` form right after `ESC (` is
/// that designator, so `enter` is not a switch sequence; extraction keeps the
/// designator ESC and the rest of `enter` as printed text, and only `leave`
/// is removed. The expected ring below holds the designator ESC and the
/// text, and the cut the removed `leave` derives closes nothing.
#[test]
fn round4_48caec6f_awaiting_designator_at_a_cut_writes_the_consumed_esc() {
    let bel = b"\x07".to_vec();
    let query = COLOR_QUERY.to_vec();
    for (enter, leave) in switch_pairs() {
        let form = String::from_utf8_lossy(enter).into_owned();
        // The task example up to the BEL.
        let open = [b"X\x1b(".as_slice(), enter, leave, b"\x1b(\x1b]11;?"].concat();
        // The ring after the fix: the example without the removed `leave`
        // switch; `enter` is the designator ESC after the waiting `ESC (`
        // plus printed text.
        let ring_of_open = [b"X\x1b(".as_slice(), enter, b"\x1b(\x1b]11;?"].concat();

        // (layout name, reads, answers the raw stream gives)
        let layouts: Vec<(&str, Vec<Vec<u8>>, usize)> = vec![
            ("BEL in a later read", vec![open.clone(), bel.clone()], 0),
            (
                "BEL in the same read",
                vec![[open.as_slice(), &bel].concat()],
                0,
            ),
            (
                "BEL in the same read, a later query",
                vec![[open.as_slice(), &bel].concat(), query.clone()],
                1,
            ),
            (
                "BEL in a later read, then a later query",
                vec![open.clone(), bel.clone(), query.clone()],
                1,
            ),
        ];
        for (layout, chunks, expected_answers) in layouts {
            let expected_ring: Vec<u8> = [ring_of_open.as_slice()]
                .into_iter()
                .chain(chunks.iter().enumerate().map(|(i, c)| {
                    if i == 0 {
                        &c[open.len()..]
                    } else {
                        c.as_slice()
                    }
                }))
                .collect::<Vec<&[u8]>>()
                .concat();
            let (_reference, reference_responses) = reference_view(&chunks);
            assert_eq!(
                r2_count(&reference_responses, COLOR_ANSWER),
                expected_answers,
                "{form} {layout}: the raw stream answers only a later complete query"
            );

            for restore_read in 0..chunks.len() {
                let ctx = format!("{form} {layout}, restore at read {restore_read}");
                let run = run_visibility_restore_at(&chunks, restore_read);
                let (_client, responses) = client_view(&run.received);
                assert_eq!(
                    responses, reference_responses,
                    "{ctx}: the client's responses equal the raw-stream reference's"
                );
                assert_eq!(
                    r2_count(&responses, COLOR_ANSWER),
                    expected_answers,
                    "{ctx}: no OSC 11 answer for the example; a later query is answered once"
                );
                assert_eq!(
                    run.ring, expected_ring,
                    "{ctx}: the ring holds the designator ESC after the waiting `ESC (`"
                );
            }
        }
    }
}

// ── AC-2 (FR3, TM-1, TS-6): one ESC on every path ────────────────────────

/// AC-2 (FR3), path 1: a cut in the same call after a run ending in
/// `ESC (` / `ESC )` writes exactly one ESC after the bytes already emitted
/// and clears the flag.
#[test]
fn round4_cut_in_the_same_call_after_a_waiting_esc_brace_writes_one_esc() {
    for brace in braces() {
        let run = [b'a', b'b', ESC, brace];
        let label = String::from_utf8_lossy(&run).into_owned();

        let mut f = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut f, &run, &[run.len()]);
        assert_eq!(
            outcome.bytes,
            [b'a', b'b', ESC, brace, ESC],
            "{label:?}: the waiting designator introducer is emitted, then one ESC"
        );
        assert!(f.pending().is_empty());
        assert!(
            !f.awaiting_designator(),
            "{label:?}: the cut clears the wait"
        );
        assert!(outcome.carried.is_none());
        assert_eq!(outcome.dims, DIMS);
        assert_at_ground_after(&outcome.bytes, &label);

        // The next ESC is an introducer again.
        let (_d, out) = f.feed(b"\x1b]0;t", DIMS);
        assert!(out.is_empty(), "{label:?}: the open OSC is held");
        assert_eq!(f.pending(), b"\x1b]0;t".as_slice());

        // Bytes after the cut start from ground: the closing ESC is written
        // after the strip, so it never opens the launch that follows.
        let mut fed = run.to_vec();
        fed.extend_from_slice(LAUNCH);
        fed.push(b'z');
        let mut g = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut g, &fed, &[run.len()]);
        assert_eq!(
            outcome.bytes,
            [b'a', b'b', ESC, brace, ESC, b'z'],
            "{label:?}: the launch after the cut is stripped, the closing ESC is kept"
        );

        // Earlier complete constructs in the run do not change the rule.
        let busy = [b"a\x1b]0;t\x07\x1b[31m\x1b(B".as_slice(), &[ESC, brace]].concat();
        let mut h = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut h, &busy, &[busy.len()]);
        assert_eq!(outcome.bytes, [busy.clone(), vec![ESC]].concat());
        assert!(!h.awaiting_designator());

        // An `ESC` that aborts an open string opens the designation: the run
        // still ends waiting, so the closing ESC is written. The string
        // closed by `ESC \` before it is complete and kept.
        for before in [&b"\x1b]0;x"[..], &b"\x1bPq1"[..], &b"\x1b]0;x\x1b\\"[..]] {
            let run = [before, &[ESC, brace]].concat();
            let mut i = ScrollbackWriteFilter::new();
            let outcome = cut_feed(&mut i, &run, &[run.len()]);
            assert_eq!(
                outcome.bytes,
                [run.clone(), vec![ESC]].concat(),
                "{:?}",
                String::from_utf8_lossy(&run)
            );
            assert!(i.pending().is_empty());
            assert!(!i.awaiting_designator());
        }
    }
}

/// AC-2 (FR3), path 1: every segment that ends awaiting a designator and is
/// followed by a cut gets its own ESC; repeated cuts at one position write it
/// once.
#[test]
fn round4_each_waiting_segment_before_a_cut_writes_its_own_esc_once() {
    // `a ESC (` | `b ESC )` | `c`: the first two segments end awaiting.
    let fed = b"a\x1b(b\x1b)c";
    let mut f = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut f, fed, &[3, 6]);
    assert_eq!(outcome.bytes, b"a\x1b(\x1bb\x1b)\x1bc".to_vec());
    assert!(!f.awaiting_designator());

    // The same cut position twice: the wait ends at the first cut.
    let mut g = ScrollbackWriteFilter::new();
    let run = b"ab\x1b(";
    let outcome = cut_feed(&mut g, run, &[run.len(), run.len()]);
    assert_eq!(outcome.bytes, b"ab\x1b(\x1b".to_vec(), "exactly one ESC");

    // A cut before the run, then a wait in the last segment: no cut follows
    // the wait, so it is carried and no ESC is written.
    let mut h = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut h, b"x\x1b(", &[0]);
    assert_eq!(outcome.bytes, b"x\x1b(".to_vec());
    assert!(h.awaiting_designator());
}

/// AC-2 (FR3), path 2: a cut at fed 0 after an earlier read left the wait
/// with an empty `pending`, with an empty fed range.
#[test]
fn round4_cut_at_fed_zero_with_an_empty_range_after_an_earlier_wait_writes_one_esc() {
    for brace in braces() {
        let mut f = ScrollbackWriteFilter::new();
        let (_d, out) = f.feed(&[b'x', ESC, brace], (80, 24));
        assert_eq!(out, [b'x', ESC, brace]);
        assert!(f.awaiting_designator());
        assert!(f.pending().is_empty());

        // The pane's dims changed since: the closing ESC is attributed to the
        // dims of this call, like every byte emitted from an empty `pending`.
        let outcome = f.feed_with_cuts(b"", (100, 30), &[0]);
        assert_eq!(outcome.bytes, vec![ESC]);
        assert_eq!(outcome.dims, (100, 30));
        assert!(!f.awaiting_designator());
        assert!(f.pending().is_empty());
        assert!(outcome.carried.is_none());
        assert_at_ground_after(&[b'x', ESC, brace, ESC], "empty range");

        // The next ESC is an introducer again.
        let (_d, out) = f.feed(b"\x1b]11;?", DIMS);
        assert!(out.is_empty());
        assert_eq!(f.pending(), b"\x1b]11;?".as_slice());
    }
}

/// AC-2 (FR3), path 2: the same cut at fed 0 with a non-empty fed range: the
/// ESC comes first, and the fed bytes start from ground.
#[test]
fn round4_cut_at_fed_zero_with_a_non_empty_range_after_an_earlier_wait_writes_one_esc() {
    for brace in braces() {
        let earlier = [b'x', ESC, brace];

        let mut f = ScrollbackWriteFilter::new();
        f.feed(&earlier, DIMS);
        let outcome = cut_feed(&mut f, b"abc", &[0]);
        assert_eq!(outcome.bytes, b"\x1babc".to_vec());
        assert!(!f.awaiting_designator());
        assert!(f.pending().is_empty());
        let mut replay = earlier.to_vec();
        replay.extend_from_slice(&outcome.bytes);
        assert_at_ground_after(&replay, "fed abc");

        // The fed range starts with a launch: after the cut it starts from
        // ground, so it is stripped, and the ESC does not open it.
        let mut g = ScrollbackWriteFilter::new();
        g.feed(&earlier, DIMS);
        let mut fed = LAUNCH.to_vec();
        fed.push(b'z');
        let outcome = cut_feed(&mut g, &fed, &[0]);
        assert_eq!(outcome.bytes, b"\x1bz".to_vec());

        // Two cuts at fed 0: the first writes the ESC, the second nothing.
        let mut h = ScrollbackWriteFilter::new();
        h.feed(&earlier, DIMS);
        let outcome = cut_feed(&mut h, b"abc", &[0, 0]);
        assert_eq!(outcome.bytes, b"\x1babc".to_vec());

        // A later cut in the same call: the wait ended at the first one.
        let mut i = ScrollbackWriteFilter::new();
        i.feed(&earlier, DIMS);
        let outcome = cut_feed(&mut i, b"abc", &[0, 3]);
        assert_eq!(outcome.bytes, b"\x1babc".to_vec());
    }
}

/// AC-2 (FR3), path 3: a cut after an overflow flush whose run ended waiting.
#[test]
fn round4_cut_after_an_overflow_flush_that_ended_waiting_writes_one_esc() {
    let pad = vec![b'p'; SCROLLBACK_FILTER_PENDING_CAP + 1];
    for brace in braces() {
        let mut run = pad.clone();
        run.extend_from_slice(&[ESC, brace]);

        // The run alone exceeds the cap; the cut follows it.
        let mut f = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut f, &run, &[run.len()]);
        assert_same_bytes(
            &outcome.bytes,
            &[run.clone(), vec![ESC]].concat(),
            "overflow then cut",
        );
        assert!(f.pending().is_empty());
        assert!(!f.awaiting_designator(), "the cut clears the wait");
        assert!(outcome.carried.is_none());
        assert_at_ground_after(&outcome.bytes, "overflow then cut");

        // Bytes after the cut start from ground.
        let mut g = ScrollbackWriteFilter::new();
        let mut fed = run.clone();
        fed.extend_from_slice(b"tail");
        let outcome = cut_feed(&mut g, &fed, &[run.len()]);
        assert_same_bytes(
            &outcome.bytes,
            &[run.clone(), vec![ESC], b"tail".to_vec()].concat(),
            "overflow then cut, bytes after the cut",
        );

        // A wait carried into the flushing call: the first fed byte is the
        // designator and is skipped by the strip; the run still ends waiting.
        let mut h = ScrollbackWriteFilter::new();
        h.feed(&[ESC, brace], DIMS);
        assert!(h.awaiting_designator());
        let outcome = cut_feed(&mut h, &run, &[run.len()]);
        assert_same_bytes(
            &outcome.bytes,
            &[run.clone(), vec![ESC]].concat(),
            "overflow with a carried wait, then cut",
        );
        assert!(!h.awaiting_designator());
    }

    // The flushed run absorbed an earlier held OSC that the trailing
    // `ESC (` aborts: the run ends waiting.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b]0;", DIMS);
    let mut fed = pad;
    fed.extend_from_slice(b"\x1b(");
    let outcome = cut_feed(&mut f, &fed, &[fed.len()]);
    assert_eq!(
        outcome.bytes.last(),
        Some(&ESC),
        "one ESC after the flushed run"
    );
    assert_eq!(outcome.bytes.len(), 4 + fed.len() + 1);
    assert!(f.pending().is_empty());
    assert!(!f.awaiting_designator());
}

/// AC-2 (FR3), path 4 at reader level: the reader's fallback closing (an
/// empty fed range with one cut at fed 0) after a read that ended in an
/// awaiting `ESC (` / `ESC )` leaves that ESC in the ring.
///
/// The fallback read is a switch form extraction does not recognize
/// (`?1049;1h` / `?47;1h`) that the shadow parser still applies, so the scan
/// and the shadow disagree about the alternate screen and the reader closes
/// instead of feeding the chunk. The pre-fix closing writes nothing for the
/// chunk: the ring ends in the waiting `ESC (`.
#[test]
fn round4_fallback_closing_after_a_waiting_esc_brace_leaves_the_esc_in_the_ring() {
    for brace in braces() {
        for form in [&b"\x1b[?1049;1h"[..], &b"\x1b[?47;1h"[..]] {
            let ctx = format!(
                "ESC {} then {}",
                brace as char,
                String::from_utf8_lossy(form)
            );
            let chunks = vec![[b'a', b'b', ESC, brace].to_vec(), form.to_vec()];
            let ring = run_reader_without_owner(chunks);
            assert_eq!(
                ring,
                [b'a', b'b', ESC, brace, ESC],
                "{ctx}: the fallback closing writes the designator ESC and nothing else for the chunk"
            );
        }
    }
}

// ── AC-3 (FR3, EC-4): nothing extra when no designator is awaited ────────

/// AC-3 (FR3, EC-4): a complete designation before a cut writes no extra ESC.
#[test]
fn round4_a_complete_designation_before_a_cut_writes_no_extra_esc() {
    for brace in braces() {
        let run = [b'a', b'b', ESC, brace, b'A'];
        let mut f = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut f, &run, &[run.len()]);
        assert_eq!(outcome.bytes, run.to_vec(), "ESC {} A", brace as char);
        assert!(!f.awaiting_designator());
        assert_at_ground_after(&outcome.bytes, "complete designation");

        // The designator itself is an ESC: still a complete designation.
        let run = [b'a', ESC, brace, ESC];
        let mut g = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut g, &run, &[run.len()]);
        assert_eq!(outcome.bytes, run.to_vec());
        assert_at_ground_after(&outcome.bytes, "designator ESC");

        // With bytes after the cut.
        let mut h = ScrollbackWriteFilter::new();
        let mut fed = [ESC, brace, b'B'].to_vec();
        fed.extend_from_slice(b"tail");
        let outcome = cut_feed(&mut h, &fed, &[3]);
        assert_eq!(outcome.bytes, fed);
    }
}

/// AC-3 (FR3, EC-4): a run ending in an open string closed by the cut writes
/// nothing extra: the construct is dropped, and no designator is awaited. A
/// brace inside a string body is string data and sets no wait either.
#[test]
fn round4_a_run_ending_in_an_open_string_closed_by_the_cut_writes_no_extra_esc() {
    for (name, open) in [
        ("OSC", &b"\x1b]0;title"[..]),
        ("OSC with a brace in its body", &b"\x1b]0;x("[..]),
        ("DCS", &b"\x1bPq#0;2"[..]),
        ("DCS with a brace in its body", &b"\x1bPq)"[..]),
        ("APC", &b"\x1b_Gi=1;PAY"[..]),
        ("lone ESC", &b"\x1b"[..]),
    ] {
        let fed = [b"ab\x1b(B".as_slice(), open].concat();
        let mut f = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut f, &fed, &[fed.len()]);
        assert_eq!(
            outcome.bytes,
            b"ab\x1b(B".to_vec(),
            "{name}: the construct is dropped and nothing is written for it"
        );
        assert!(f.pending().is_empty());
        assert!(!f.awaiting_designator());
        assert_at_ground_after(&outcome.bytes, name);

        // Carried in `pending` from an earlier read, closed by a cut at fed 0.
        let mut g = ScrollbackWriteFilter::new();
        let (_d, first) = g.feed(open, DIMS);
        assert!(first.is_empty(), "{name}: held back");
        let outcome = cut_feed(&mut g, b"", &[0]);
        assert!(
            outcome.bytes.is_empty(),
            "{name}: dropped without a closing write"
        );
        assert!(g.pending().is_empty());
    }
}

/// AC-3 (FR3, EC-4): a cut with nothing held and no wait writes nothing.
#[test]
fn round4_a_cut_with_nothing_held_and_no_wait_writes_nothing() {
    let mut f = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut f, b"abc", &[3]);
    assert_eq!(outcome.bytes, b"abc".to_vec());

    // An empty range with a cut on a fresh filter, and after a plain read.
    let mut g = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut g, b"", &[0]);
    assert!(outcome.bytes.is_empty());
    let mut h = ScrollbackWriteFilter::new();
    h.feed(b"plain", DIMS);
    let outcome = cut_feed(&mut h, b"", &[0]);
    assert!(outcome.bytes.is_empty());

    // After a complete designation from an earlier read.
    let mut i = ScrollbackWriteFilter::new();
    i.feed(b"x\x1b(B", DIMS);
    let outcome = cut_feed(&mut i, b"", &[0]);
    assert!(outcome.bytes.is_empty());

    // Many cuts with nothing to close.
    let mut j = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut j, b"", &[0, 0, 0]);
    assert!(outcome.bytes.is_empty());
}

/// AC-3 (FR3, EC-4): a designator carried in from an earlier read and
/// consumed by the segment's first byte leaves nothing to wait for at a later
/// cut.
#[test]
fn round4_a_carried_designator_consumed_by_the_first_byte_writes_no_extra_esc_at_a_later_cut() {
    for brace in braces() {
        let earlier = [b'x', ESC, brace];

        // A plain designator byte and bytes after it.
        let mut f = ScrollbackWriteFilter::new();
        f.feed(&earlier, DIMS);
        let outcome = cut_feed(&mut f, b"Bcd", &[3]);
        assert_eq!(outcome.bytes, b"Bcd".to_vec());
        assert!(!f.awaiting_designator());

        // The segment is the designator byte alone.
        let mut g = ScrollbackWriteFilter::new();
        g.feed(&earlier, DIMS);
        let outcome = cut_feed(&mut g, b"B", &[1]);
        assert_eq!(outcome.bytes, b"B".to_vec());

        // The designator is an ESC.
        let mut h = ScrollbackWriteFilter::new();
        h.feed(&earlier, DIMS);
        let outcome = cut_feed(&mut h, &[ESC], &[1]);
        assert_eq!(outcome.bytes, vec![ESC]);
        let mut replay = earlier.to_vec();
        replay.extend_from_slice(&outcome.bytes);
        assert_at_ground_after(&replay, "designator ESC");

        // The designator ESC does not open the launch behind it.
        let mut i = ScrollbackWriteFilter::new();
        i.feed(&earlier, DIMS);
        let mut fed = LAUNCH.to_vec();
        fed.extend_from_slice(b"z");
        let outcome = cut_feed(&mut i, &fed, &[fed.len()]);
        assert_eq!(outcome.bytes, fed);
    }
}

/// AC-3 (FR3): an overflow flush in the LAST segment is followed by no cut:
/// no closing is written and the wait is carried instead.
#[test]
fn round4_overflow_flush_in_the_last_segment_writes_no_closing_esc() {
    let pad = vec![b'p'; SCROLLBACK_FILTER_PENDING_CAP + 1];
    for brace in braces() {
        let mut run = pad.clone();
        run.extend_from_slice(&[ESC, brace]);
        let mut f = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut f, &run, &[]);
        assert_same_bytes(&outcome.bytes, &run, "overflow flush in the last segment");
        assert!(f.awaiting_designator(), "the wait is carried");
        assert!(f.pending().is_empty());

        // A cut that only closes the previous segment writes for that
        // segment; the last (flushed) segment still carries its wait.
        let mut g = ScrollbackWriteFilter::new();
        let mut fed = b"a".to_vec();
        fed.extend_from_slice(&run);
        let outcome = cut_feed(&mut g, &fed, &[1]);
        assert_same_bytes(
            &outcome.bytes,
            &fed,
            "no wait before the cut: nothing extra",
        );
        assert!(g.awaiting_designator());
    }
}

/// AC-3 (FR3): an overflow flush followed by a cut writes nothing extra when
/// the flushed run does not end waiting and ends in ground: in a complete
/// designation and in plain text.
#[test]
fn round4_overflow_flush_then_a_cut_without_a_wait_writes_nothing_extra() {
    let pad = vec![b'p'; SCROLLBACK_FILTER_PENDING_CAP + 1];

    // Ends in a complete designation.
    let mut run = pad.clone();
    run.extend_from_slice(b"\x1b(B");
    let mut f = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut f, &run, &[run.len()]);
    assert_same_bytes(
        &outcome.bytes,
        &run,
        "complete designation after an overflow",
    );
    assert!(!f.awaiting_designator());

    // Ends in plain text.
    let mut g = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut g, &pad, &[pad.len()]);
    assert_same_bytes(&outcome.bytes, &pad, "plain overflow");
}

// ── AC-4 (EC-5): the switch sits in the designator slot ──────────────────

/// AC-4 (FR3, EC-5, TM-1): the `ESC (` / `ESC )` that ends the previous read
/// is followed, at the start of the next read, by a 47 / 1047 / 1049 `h` /
/// `l` pair and then `ESC <brace> ESC ]11;?`; a BEL and a later complete
/// query follow in later reads. Whatever extraction does with the switch, the
/// client's responses equal the raw-stream reference's: the BEL completes no
/// OSC, and the later query is answered once. Display differences that come
/// from FR5's path are not asserted.
#[test]
fn round4_ec5_a_switch_in_the_designator_slot_never_fabricates_a_query() {
    for brace in braces() {
        for (enter, leave) in switch_pairs() {
            let form = String::from_utf8_lossy(enter).into_owned();
            let chunks = vec![
                vec![b'X', ESC, brace],
                [enter, leave, &[ESC, brace, ESC], b"]11;?"].concat(),
                b"\x07".to_vec(),
                COLOR_QUERY.to_vec(),
            ];
            let (_reference, reference_responses) = reference_view(&chunks);
            assert_eq!(
                r2_count(&reference_responses, COLOR_ANSWER),
                1,
                "the raw stream answers only the later query"
            );
            for restore_read in 1..chunks.len() {
                let ctx = format!(
                    "ESC {} then {form}, restore at read {restore_read}",
                    brace as char
                );
                let run = run_visibility_restore_at(&chunks, restore_read);
                let (_client, responses) = client_view(&run.received);
                assert_eq!(
                    responses, reference_responses,
                    "{ctx}: the client's responses equal the raw-stream reference's"
                );
            }
        }
    }
}

// ── AC-5 (TM-2, NFR3, NFR5): the closing write is O(1) ───────────────────

/// AC-5 (TM-2, NFR3, NFR5): long designator chains with many cuts finish
/// within the budget; each waiting segment gets exactly one ESC.
#[test]
fn round4_long_designator_chains_with_many_cuts_finish_within_the_budget() {
    let pairs = 30_000usize;

    // `ESC (` | `ESC (` | ...: every segment ends waiting.
    let fed: Vec<u8> = [ESC, b'('].repeat(pairs);
    let cuts: Vec<usize> = (1..=pairs).map(|k| 2 * k).collect();
    let start = std::time::Instant::now();
    let mut f = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut f, &fed, &cuts);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_same_bytes(
        &outcome.bytes,
        &[ESC, b'(', ESC].repeat(pairs),
        "waiting segments",
    );
    assert!(f.pending().is_empty());
    assert!(!f.awaiting_designator());

    // `ESC ( ESC` | ...: the designator is present, nothing awaited.
    let fed: Vec<u8> = [ESC, b'(', ESC].repeat(pairs);
    let cuts: Vec<usize> = (1..=pairs).map(|k| 3 * k).collect();
    let start = std::time::Instant::now();
    let mut g = ScrollbackWriteFilter::new();
    let outcome = cut_feed(&mut g, &fed, &cuts);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_same_bytes(&outcome.bytes, &fed, "no extra ESC when nothing is awaited");

    // One long chain in one segment, then a cut: one ESC exactly when the
    // chain ends waiting (an odd number of pairs), and the client is at
    // ground afterwards either way.
    for pairs in [30_000usize, 30_001] {
        let fed: Vec<u8> = [ESC, b'('].repeat(pairs);
        let start = std::time::Instant::now();
        let mut h = ScrollbackWriteFilter::new();
        let outcome = cut_feed(&mut h, &fed, &[fed.len()]);
        assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
        let waiting = pairs % 2 == 1;
        assert_eq!(
            outcome.bytes.len(),
            fed.len() + usize::from(waiting),
            "{pairs} pairs"
        );
        assert_at_ground_after(&outcome.bytes, &format!("{pairs} pairs"));
    }

    // Many cuts at one position, over many calls.
    let start = std::time::Instant::now();
    let mut j = ScrollbackWriteFilter::new();
    for _ in 0..pairs {
        let outcome = cut_feed(&mut j, &[ESC, b'('], &[2, 2, 2]);
        assert_eq!(outcome.bytes, [ESC, b'(', ESC]);
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-5 (TM-2, NFR3, NFR5): switch sequences following a waiting `ESC (` over
/// many reads — both the recognized form (cuts) and the fallback form —
/// finish within the budget and leave exactly one closing ESC per wait.
///
/// round4 FR5 (task0005): in the recognized form the first ESC of the next
/// read is the designator, not a switch ESC. The ring keeps it and the text
/// after it (`ESC [?1049h`) and removes only the `leave` switch, so no
/// closing ESC is written and none is needed.
#[test]
fn round4_many_straddling_switches_after_a_waiting_designator_stay_within_the_budget() {
    let rounds = 400usize;

    // Recognized form: the designator ESC and the printed `[?1049h` stay in
    // the ring, the `leave` switch is removed.
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..rounds {
        chunks.push(b"x\x1b(".to_vec());
        chunks.push([b"\x1b[?1049h".as_slice(), b"\x1b[?1049l"].concat());
    }
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(chunks);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_same_bytes(
        &ring,
        &b"x\x1b(\x1b[?1049h".repeat(rounds),
        "recognized form",
    );

    // Fallback form: the shadow parser enters and leaves the alternate
    // screen on forms extraction does not recognize.
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..rounds {
        chunks.push(b"x\x1b(".to_vec());
        chunks.push(b"\x1b[?1049;1h".to_vec());
        chunks.push(b"\x1b[?1049;1l".to_vec());
    }
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(chunks);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_same_bytes(&ring, &b"x\x1b(\x1b".repeat(rounds), "fallback form");
}

/// AC-5 (NFR5): through the suppressed pipeline every chunk the reader sends
/// other than the EOF marker is non-empty, and the client's responses equal
/// the raw-stream reference's.
#[test]
fn round4_closing_writes_never_make_the_reader_send_an_empty_chunk() {
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..2 {
        chunks.push(b"x\x1b(".to_vec());
        chunks.push([b"\x1b[?1049h".as_slice(), b"\x1b[?1049l"].concat());
    }
    chunks.push(b"x\x1b(".to_vec());
    chunks.push(b"\x1b[?1049;1h".to_vec());
    chunks.push(b"\x1b[?1049;1l".to_vec());
    chunks.push(COLOR_QUERY.to_vec());
    let last = chunks.len() - 1;
    let start = std::time::Instant::now();
    let run = run_reader_with_suppressed_reads(&chunks, &[last]);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    let (eof, rest) = run.received.split_last().expect("deliveries");
    assert!(eof.data.is_empty(), "the last chunk is the EOF marker");
    for chunk in rest {
        assert!(
            !chunk.data.is_empty(),
            "every chunk other than the EOF marker is non-empty"
        );
    }
}
