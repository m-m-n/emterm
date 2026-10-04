//! mux-write-filter-overflow-lone-esc task0001 (FR1-FR6, NFR1-NFR3): the
//! overflow flush of the scrollback write filter holds the final live lone
//! `ESC` of its run instead of writing it.
//!
//! The write filter never holds the `ESC` of a run it flushes at the 512 KiB
//! cap, so a strip target split right after that `ESC` (`ESC` in this call,
//! `[6n` in the next) reached the ring in executable form: the strip saw the
//! `ESC` alone, then `[6n` alone, and removed neither. The flush now holds the
//! run's last byte in `pending` when it is an `ESC` that leaves the written
//! stream in the Escape state and the flush is in the call's last segment, so
//! the next call strips it together with its continuation, as the non-overflow
//! path already does.
//!
//! Oracle convention (the one mux-strip-escape-state-carry uses): replay
//! checks compare term_core views (rows, cursor, responses); for a cut the
//! reference is term_core fed the raw stream with a 47 / 1047 / 1049 `h` / `l`
//! pair in place of the cut, the removed construct's own effect kept out
//! (`view_after_a_cut`). Exact-bytes checks compare against the write-path
//! strip. The held run is about 0.5 MiB per case: each test builds it once and
//! clones it per case.
//!
//! Test identifiers: AC-1 the reproduction, AC-2 the removal and the byte
//! totals, AC-3 the held state, AC-4 the continuations, AC-5 the cut and the
//! fallback closing, AC-6 the overflow cases outside the hold (regression
//! pins), AC-7 the production reader.

use super::post_strip_cut_csi::{
    AGENT_STATUS, BUDGET, KITTY, LAUNCH, MD_LAUNCH, SIXEL, all_targets,
};
use super::round3_write_path::{DIMS, run_reader_without_owner, switch_pairs};
use super::round4_cut_csi::{osc_held_at_the_cap, text, view_after_a_cut, view_of};
use super::*;
use crate::mux::scrollback_filter::strip_pty_output_for_scrollback_write;
use crate::mux::scrollback_filter::tests::client_written_state;

const ESC: &[u8] = &[0x1b];
const BEL: &[u8] = &[0x07];

// ── helpers ──────────────────────────────────────────────────────────────

/// Whether `needle` occurs in `haystack`.
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The part of AC-1's call 1 before its final `ESC`: ten body bytes of the held
/// OSC, its BEL and text.
fn ten_bel_abc() -> Vec<u8> {
    [&b"pppppppppp"[..], BEL, b"abc"].concat()
}

/// One way the overflowing call 1 can end in a live lone `ESC`.
struct Ending {
    name: &'static str,
    /// Call 1, final `ESC` included.
    call1: Vec<u8>,
    /// Whether a written state is that of the bytes the flush writes before that
    /// final `ESC`. A predicate, because an open CSI's state carries a
    /// classification the tests cannot build; its phase is what is pinned.
    state_before_esc: fn(WrittenState) -> bool,
    /// What the reader's fallback closing writes once the held `ESC` is dropped.
    closing: &'static [u8],
}

impl Ending {
    /// Call 1 without its final `ESC`.
    fn call1_without_esc(&self) -> &[u8] {
        assert_eq!(self.call1.last(), Some(&0x1b), "{}", self.name);
        &self.call1[..self.call1.len() - 1]
    }
}

/// AC-1's ending first, then AC-2's three extra endings.
fn endings() -> Vec<Ending> {
    let plain = ten_bel_abc();
    vec![
        Ending {
            name: "after plain text",
            call1: [&plain[..], ESC].concat(),
            state_before_esc: |s| s == WrittenState::Ground,
            closing: &[],
        },
        Ending {
            name: "after a written ESC",
            call1: [&plain[..], ESC, ESC].concat(),
            state_before_esc: |s| s == WrittenState::Escape,
            closing: ESCAPE_CLOSING,
        },
        Ending {
            name: "after an open CSI",
            call1: [&plain[..], b"\x1b[6", ESC].concat(),
            state_before_esc: |s| s.csi() == Some(CsiPhase::Param),
            closing: CSI_CLOSING,
        },
        Ending {
            name: "inside an open string body",
            call1: [&b"pppppppppp"[..], ESC].concat(),
            state_before_esc: |s| s == WrittenState::Ground,
            closing: &[],
        },
    ]
}

/// The call-2 continuations of AC-2, each without its opening `ESC`.
fn continuations() -> Vec<(&'static str, Vec<u8>)> {
    let mut all: Vec<(&'static str, Vec<u8>)> = vec![
        ("csi 6n", b"[6n".to_vec()),
        ("csi 5n", b"[5n".to_vec()),
        ("csi c", b"[c".to_vec()),
    ];
    for (name, construct) in [
        ("osc 777 viewer launch", LAUNCH),
        ("osc 9999 emterm-md launch", MD_LAUNCH),
        ("osc 777 agent-status report", AGENT_STATUS),
        ("kitty apc", KITTY),
        ("sixel dcs", SIXEL),
    ] {
        assert_eq!(construct[0], 0x1b, "{name}");
        all.push((name, construct[1..].to_vec()));
    }
    all
}

/// A fresh filter fed `held` (the OSC held at the cap: nothing is written) and
/// then `call1` in one cut-free call, which takes the overflow flush.
fn overflow(held: &[u8], call1: &[u8]) -> (ScrollbackWriteFilter, FeedOutcome) {
    let mut filter = ScrollbackWriteFilter::new();
    assert!(
        filter.feed(held, DIMS).1.is_empty(),
        "the OSC held at the cap is still held"
    );
    assert_eq!(filter.pending_len(), SCROLLBACK_FILTER_PENDING_CAP);
    let outcome = filter.feed_with_cuts(call1, DIMS, &[]);
    assert!(outcome.carried.is_none(), "the flushed call reports none");
    (filter, outcome)
}

/// The write-path strip of the held OSC and `call1` without its final `ESC`:
/// what the overflow flush writes when it holds that `ESC`.
fn written_without_the_final_esc(held: &[u8], ending: &Ending) -> Vec<u8> {
    strip_pty_output_for_scrollback_write(&[held, ending.call1_without_esc()].concat())
}

/// Every byte written when the held OSC, `call1` without its final `ESC`, and
/// then that `ESC` together with the continuation (one call that does not
/// overflow) are fed to a fresh filter: the reference feeding of AC-2.
fn reference_total(held: &[u8], call1_without_esc: &[u8], esc_and_continuation: &[u8]) -> Vec<u8> {
    let mut filter = ScrollbackWriteFilter::new();
    assert!(filter.feed(held, DIMS).1.is_empty());
    let mut total = filter.feed(call1_without_esc, DIMS).1;
    total.extend_from_slice(&filter.feed(esc_and_continuation, DIMS).1);
    total
}

// ── AC-1 (FR1, FR6, TM-1): the reproduction ──────────────────────────────

/// AC-1 (SPEC AC-1; FR1, FR6, TM-1): the OSC held at the cap, then a call of
/// ten body bytes, BEL, `abc` and a final `ESC` (the overflow flush), then a
/// call of `[6n`. No `ESC[6n` is written, and term_core fed the written bytes
/// reports no cursor position.
#[test]
fn overflow_lone_esc_a_split_cursor_position_query_is_not_written_in_executable_form() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    let (mut filter, outcome) = overflow(&held, &ending.call1);
    let later = filter.feed(b"[6n", DIMS).1;
    let total = [&outcome.bytes[..], &later[..]].concat();
    assert!(
        !contains_bytes(&total, b"\x1b[6n"),
        "the written bytes hold no ESC[6n"
    );
    assert!(
        view_of(&total).responses.is_empty(),
        "term_core fed the written bytes reports no cursor position"
    );
}

// ── AC-2 (FR1, NFR2, TM-1): every strip target, four endings ─────────────

/// AC-2 (SPEC AC-2; FR1, NFR2, TM-1): for each continuation after each of the
/// four call-1 endings, the construct is absent from the written bytes, and
/// the written bytes in total equal those of the reference feeding (the same
/// held OSC, call 1 without its final `ESC`, then that `ESC` and the
/// continuation together in a call that does not overflow).
#[test]
fn overflow_lone_esc_every_continuation_is_removed_and_the_total_equals_the_reference() {
    let start = std::time::Instant::now();
    let held = osc_held_at_the_cap();
    for ending in endings() {
        for (name, continuation) in continuations() {
            let ctx = format!("{}, continuation {name}", ending.name);
            let (mut filter, outcome) = overflow(&held, &ending.call1);
            let later = filter.feed(&continuation, DIMS).1;
            let total = [&outcome.bytes[..], &later[..]].concat();

            let construct = [ESC, &continuation[..]].concat();
            assert!(
                !contains_bytes(&total, &construct),
                "{ctx}: the construct is absent from the written bytes"
            );
            let reference = reference_total(&held, ending.call1_without_esc(), &construct);
            assert!(
                total == reference,
                "{ctx}: the written bytes equal the reference feeding's"
            );
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-3 (FR2, FR3, FR5, TM-2): the held state ───────────────────────────

/// AC-3 (SPEC AC-3; FR2, FR3, TM-2): right after AC-1's call 1 the filter is in
/// the state of a lone trailing `ESC` held by the non-overflow path.
#[test]
fn overflow_lone_esc_the_hold_leaves_the_state_of_a_held_lone_esc() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    let (filter, outcome) = overflow(&held, &ending.call1);

    assert_eq!(filter.pending(), ESC, "exactly the one ESC is held");
    assert_eq!(filter.pending_len(), 1);
    assert_eq!(
        filter.held_construct_start(),
        Some(0),
        "a chain of one construct"
    );
    assert!(
        outcome.bytes == written_without_the_final_esc(&held, ending),
        "call 1 writes the strip of the held OSC and call 1 without its final ESC"
    );
    assert_eq!(
        filter.written_state(),
        client_written_state(&outcome.bytes),
        "the written state is term_core's end state of the bytes written so far"
    );
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
    assert!(outcome.carried.is_none(), "no carried completion");
}

/// AC-3 (SPEC AC-3; FR2, TM-2): for each of the three extra endings the
/// overflowing call leaves exactly the one `ESC` in `pending`, a chain of one
/// construct, in the written state of the bytes before it.
#[test]
fn overflow_lone_esc_every_live_ending_holds_exactly_one_byte() {
    let held = osc_held_at_the_cap();
    for ending in endings() {
        let (filter, outcome) = overflow(&held, &ending.call1);
        assert_eq!(filter.pending(), ESC, "{}", ending.name);
        assert_eq!(filter.pending_len(), 1, "{}", ending.name);
        assert_eq!(filter.held_construct_start(), Some(0), "{}", ending.name);
        assert!(
            (ending.state_before_esc)(filter.written_state()),
            "{}: the written state is that of the bytes written before the ESC, got {:?}",
            ending.name,
            filter.written_state()
        );
        assert!(!filter.awaiting_designator(), "{}", ending.name);
        assert!(
            outcome.bytes == written_without_the_final_esc(&held, &ending),
            "{}: the final ESC is not written",
            ending.name
        );
    }
}

/// AC-3 (SPEC AC-3; FR5): the held OSC fed under dims A, call 1 under dims B and
/// a call 2 of `x` under dims C: call 1's outcome carries A (the dims the carried
/// run started under) and call 2's carries B (the held ESC's start dims are the
/// overflowing call's current dims).
#[test]
fn overflow_lone_esc_the_held_esc_is_attributed_to_the_overflowing_calls_dims() {
    let (a, b, c) = ((80, 24), (100, 30), (120, 40));
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    let mut filter = ScrollbackWriteFilter::new();
    assert!(filter.feed(&held, a).1.is_empty());
    let outcome = filter.feed_with_cuts(&ending.call1, b, &[]);
    assert_eq!(
        outcome.dims, a,
        "call 1 is attributed to the carried run's dims"
    );
    assert_eq!(filter.pending(), ESC);
    let outcome = filter.feed_with_cuts(b"x", c, &[]);
    assert_eq!(outcome.dims, b, "call 2 is attributed to call 1's dims");
    assert_eq!(outcome.bytes, b"\x1bx".to_vec());
}

/// AC-3 (FR1, FR2): a call that is itself past the cap, with no carried run, and
/// one that starts in a designator wait, hold their final live `ESC` too, in the
/// dims of their own call.
#[test]
fn overflow_lone_esc_a_fresh_or_designator_started_flush_holds_its_final_esc() {
    let pad = vec![b'p'; SCROLLBACK_FILTER_PENDING_CAP + 1];

    // No carried run at all.
    let mut filter = ScrollbackWriteFilter::new();
    let outcome = filter.feed_with_cuts(&[&pad[..], ESC].concat(), DIMS, &[]);
    assert!(outcome.bytes == pad, "the run without its ESC is written");
    assert_eq!(outcome.dims, DIMS);
    assert_eq!(filter.pending(), ESC);
    assert_eq!(filter.held_construct_start(), Some(0));
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
    assert!(
        filter.feed(b"[6n", DIMS).1.is_empty(),
        "the query is stripped"
    );

    // The call starts in a designator wait: its first byte is the designator.
    let mut filter = ScrollbackWriteFilter::new();
    assert_eq!(filter.feed(b"\x1b(", DIMS).1, b"\x1b(".to_vec());
    assert!(filter.awaiting_designator());
    let outcome = filter.feed_with_cuts(&[&b"B"[..], &pad[..], ESC].concat(), DIMS, &[]);
    assert!(
        outcome.bytes == [&b"B"[..], &pad[..]].concat(),
        "the designator and the run before the ESC are written"
    );
    assert_eq!(filter.pending(), ESC);
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
}

// ── AC-4 (FR4): what the next call does with the held ESC ────────────────

/// AC-4 (SPEC AC-4; FR4): a continuation that is not a strip target is written
/// as the held `ESC` followed by it, with no byte lost.
#[test]
fn overflow_lone_esc_a_non_strip_continuation_is_written_after_the_esc() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    let before = written_without_the_final_esc(&held, ending);
    for continuation in [&b"x"[..], b"[H"] {
        let (mut filter, outcome) = overflow(&held, &ending.call1);
        let later = filter.feed(continuation, DIMS).1;
        assert_eq!(
            later,
            [ESC, continuation].concat(),
            "continuation {:?}: the ESC and the continuation, whole",
            text(continuation)
        );
        let total = [&outcome.bytes[..], &later[..]].concat();
        assert!(
            total == [&before[..], ESC, continuation].concat(),
            "continuation {:?}: no byte is lost",
            text(continuation)
        );
    }
}

/// AC-4 (SPEC AC-4; FR4): a continuation that completes a string the held `ESC`
/// opens is reported as a carried completion whose bytes begin at that `ESC`.
#[test]
fn overflow_lone_esc_a_completing_continuation_is_reported_as_a_carried_completion() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    let before = written_without_the_final_esc(&held, ending);
    let (mut filter, outcome) = overflow(&held, &ending.call1);
    let second = filter.feed_with_cuts(b"]0;t\x07", DIMS, &[]);
    let completion = second.carried.expect("the held ESC's string completed");
    assert_eq!(completion.bytes(), b"\x1b]0;t\x07");
    assert_eq!(completion.fed_end(), 5);
    let total = [&outcome.bytes[..], &second.bytes[..]].concat();
    assert!(total == [&before[..], b"\x1b]0;t\x07"].concat());
    assert!(filter.pending().is_empty());
}

/// AC-4 (SPEC AC-4; FR4): after the open-string-body ending a call of `\` is
/// written as `ESC \`.
#[test]
fn overflow_lone_esc_a_backslash_after_an_open_string_body_is_written_as_st() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[3];
    assert_eq!(ending.name, "inside an open string body");
    let before = written_without_the_final_esc(&held, ending);
    let (mut filter, outcome) = overflow(&held, &ending.call1);
    let later = filter.feed(b"\\", DIMS).1;
    assert_eq!(later, b"\x1b\\".to_vec());
    let total = [&outcome.bytes[..], &later[..]].concat();
    assert!(total == [&before[..], b"\x1b\\"].concat());
    assert!(filter.pending().is_empty());
}

/// FR4 (SPEC "a next call that itself overflows"): a call that takes the
/// overflow flush itself strips the held `ESC` together with its own bytes in
/// its flushed run.
#[test]
fn overflow_lone_esc_a_next_call_that_overflows_strips_the_held_esc_with_its_bytes() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    let (mut filter, outcome) = overflow(&held, &ending.call1);
    let next = [&b"[6n"[..], &vec![b'q'; SCROLLBACK_FILTER_PENDING_CAP][..]].concat();
    let later = filter.feed(&next, DIMS).1;
    assert!(
        later == vec![b'q'; SCROLLBACK_FILTER_PENDING_CAP],
        "the held ESC and the query behind it are removed from the flushed run"
    );
    assert!(!contains_bytes(&outcome.bytes, b"\x1b[6n"));
    assert!(filter.pending().is_empty());
}

// ── AC-5 (FR4, TM-1): a cut and the reader's fallback closing ────────────

/// AC-5 (SPEC AC-5; FR4, TM-1), (a): the reader's fallback closing (an empty fed
/// range with a cut at 0) drops the held `ESC` unwritten, leaves `pending`
/// empty and the written state Ground, and a second closing writes nothing; the
/// `[6n` of a later call replays like the raw stream with the switch in place
/// of the cut, with no cursor-position report.
#[test]
fn overflow_lone_esc_the_fallback_closing_drops_the_held_esc() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    for (enter, leave) in switch_pairs() {
        let pair = [enter, leave].concat();
        let ctx = format!("switch {}", text(enter));
        let reference =
            view_after_a_cut(&[&held[..], &ending.call1[..], &pair[..]].concat(), b"[6n");
        assert!(
            reference.responses.is_empty(),
            "{ctx}: the raw stream gives no cursor position"
        );

        let (mut filter, outcome) = overflow(&held, &ending.call1);
        let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
        assert!(
            closing.is_empty(),
            "{ctx}: nothing is written for the held ESC"
        );
        assert!(filter.pending().is_empty(), "{ctx}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
        assert!(
            filter.feed_with_cuts(b"", DIMS, &[0]).bytes.is_empty(),
            "{ctx}: a second closing writes nothing"
        );
        let later = filter.feed(b"[6n", DIMS).1;
        assert_eq!(
            later,
            b"[6n".to_vec(),
            "{ctx}: the query text is written as text"
        );
        let written_before = [&outcome.bytes[..], &closing[..]].concat();
        assert_eq!(
            view_after_a_cut(&written_before, &later),
            reference,
            "{ctx}: the replay matches the raw stream with the switch for the cut"
        );
    }
}

/// AC-5 (SPEC AC-5; FR4, TM-1), (b): a call 2 of `[6n` that carries a cut at fed
/// 0 drops the held `ESC` unwritten: right after it `pending` is empty and the
/// written state is Ground, and the replay matches the raw stream with the switch
/// in place of the cut, with no cursor-position report.
#[test]
fn overflow_lone_esc_a_cut_at_fed_zero_drops_the_held_esc() {
    let held = osc_held_at_the_cap();
    let ending = &endings()[0];
    for (enter, leave) in switch_pairs() {
        let pair = [enter, leave].concat();
        let ctx = format!("switch {}", text(enter));
        let reference =
            view_after_a_cut(&[&held[..], &ending.call1[..], &pair[..]].concat(), b"[6n");
        assert!(reference.responses.is_empty(), "{ctx}");

        let (mut filter, outcome) = overflow(&held, &ending.call1);
        let second = filter.feed_with_cuts(b"[6n", DIMS, &[0]);
        assert_eq!(
            second.bytes,
            b"[6n".to_vec(),
            "{ctx}: nothing for the held ESC, then the text"
        );
        assert!(filter.pending().is_empty(), "{ctx}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
        assert!(!filter.awaiting_designator(), "{ctx}");
        assert_eq!(
            view_after_a_cut(&outcome.bytes, &second.bytes),
            reference,
            "{ctx}: the replay matches the raw stream with the switch for the cut"
        );
    }
}

/// AC-5 (SPEC AC-5; FR4, IMPLEMENTATION.md Risk row 2): the closure the fallback
/// closing writes after the held `ESC` is dropped follows the written state of
/// the bytes before it: DEL after the open-CSI ending, the Escape closure after
/// a written `ESC`, and nothing after plain text or inside an open string body.
/// After the CSI closing the later `[6n` gives no cursor-position report.
#[test]
fn overflow_lone_esc_the_fallback_closing_follows_the_written_state() {
    let held = osc_held_at_the_cap();
    for ending in endings() {
        let (mut filter, outcome) = overflow(&held, &ending.call1);
        let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
        assert_eq!(closing, ending.closing.to_vec(), "{}", ending.name);
        assert!(filter.pending().is_empty(), "{}", ending.name);
        assert_eq!(
            filter.written_state(),
            WrittenState::Ground,
            "{}",
            ending.name
        );
        assert!(
            filter.feed_with_cuts(b"", DIMS, &[0]).bytes.is_empty(),
            "{}: a second closing writes nothing",
            ending.name
        );
        if ending.name == "after an open CSI" {
            assert_eq!(closing, vec![0x7f], "exactly one DEL");
            let later = filter.feed(b"[6n", DIMS).1;
            let ring = [&outcome.bytes[..], &closing[..], &later[..]].concat();
            assert!(
                view_of(&ring).responses.is_empty(),
                "the DEL cancelled the CSI, so the later [6n is text"
            );
        }
    }
}

// ── AC-6 (FR3, NFR3): overflow cases outside the hold keep today's bytes ──

/// A fresh filter fed `held` and then `tail` in one call carrying `cuts`;
/// returns the filter, the outcome and the write-path strip of the whole flushed
/// run (held OSC and `tail`).
fn flush_after_held(
    held: &[u8],
    tail: &[u8],
    cuts: &[usize],
) -> (ScrollbackWriteFilter, FeedOutcome, Vec<u8>) {
    let mut filter = ScrollbackWriteFilter::new();
    assert!(filter.feed(held, DIMS).1.is_empty());
    let outcome = filter.feed_with_cuts(tail, DIMS, cuts);
    let stripped = strip_pty_output_for_scrollback_write(&[held, tail].concat());
    (filter, outcome, stripped)
}

/// AC-6 (SPEC AC-6; FR3, NFR3), regression pin: a run ending in `ESC` and a
/// complete removed construct (the R9 form) is written as the strip of the whole
/// run, with `pending` empty. The strip closes the written `ESC` at the removed
/// construct (mux-strip-concat-query-closure), so the state is Ground.
#[test]
fn overflow_lone_esc_a_run_ending_in_esc_and_a_removed_construct_flushes_whole() {
    let held = osc_held_at_the_cap();
    let plain = ten_bel_abc();
    for (name, target) in all_targets() {
        let tail = [&plain[..], ESC, target].concat();
        let (filter, outcome, stripped) = flush_after_held(&held, &tail, &[]);
        assert!(
            stripped.ends_with(&[&b"abc\x1b"[..], CSI_CLOSING].concat()),
            "{name}: the run is stripped"
        );
        assert!(outcome.bytes == stripped, "{name}: the whole run's strip");
        assert!(filter.pending().is_empty(), "{name}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
        assert!(!filter.awaiting_designator(), "{name}");
    }
}

/// AC-6 (SPEC AC-6; FR3, NFR3), regression pin: a run ending in `ESC ( ESC`
/// (the final `ESC` is the charset designator) is written whole, with `pending`
/// empty, the Ground state and no designator wait.
#[test]
fn overflow_lone_esc_a_run_ending_in_a_designator_esc_flushes_whole() {
    let held = osc_held_at_the_cap();
    let tail = [&ten_bel_abc()[..], b"\x1b(", ESC].concat();
    let (filter, outcome, stripped) = flush_after_held(&held, &tail, &[]);
    assert!(stripped.ends_with(b"abc\x1b(\x1b"));
    assert!(outcome.bytes == stripped);
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
}

/// AC-6 (SPEC AC-6; FR3, NFR3), regression pin: a run ending in `ESC[6` and a
/// removed construct is written as the strip of the whole run, with `pending`
/// empty. The strip closes the open CSI at the removed construct
/// (mux-strip-concat-query-closure), so the state is Ground.
#[test]
fn overflow_lone_esc_a_run_ending_in_an_open_csi_and_a_removed_construct_flushes_whole() {
    let held = osc_held_at_the_cap();
    let plain = ten_bel_abc();
    for (name, target) in all_targets() {
        let tail = [&plain[..], b"\x1b[6", target].concat();
        let (filter, outcome, stripped) = flush_after_held(&held, &tail, &[]);
        assert!(
            stripped.ends_with(&[&b"abc\x1b[6"[..], CSI_CLOSING].concat()),
            "{name}: the run is stripped"
        );
        assert!(outcome.bytes == stripped, "{name}: the whole run's strip");
        assert!(filter.pending().is_empty(), "{name}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
        assert!(!filter.awaiting_designator(), "{name}");
    }
}

/// AC-6 (SPEC AC-6; FR3, NFR3), regression pin: an overflow flush followed by a
/// cut in the same call writes the stripped run, `ESC` included, and then one
/// closure, and holds nothing: the Escape closure for a run ending in a live
/// lone `ESC`, DEL for a run ending in `ESC[6`.
#[test]
fn overflow_lone_esc_a_flush_followed_by_a_cut_writes_the_run_and_one_closure() {
    let held = osc_held_at_the_cap();
    let plain = ten_bel_abc();

    let tail = [&plain[..], ESC].concat();
    let (filter, outcome, stripped) = flush_after_held(&held, &tail, &[tail.len()]);
    assert!(stripped.ends_with(b"abc\x1b"));
    assert!(
        outcome.bytes == [&stripped[..], ESCAPE_CLOSING].concat(),
        "the stripped run, ESC included, then one Escape closure"
    );
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());

    let tail = [&plain[..], b"\x1b[6"].concat();
    let (filter, outcome, stripped) = flush_after_held(&held, &tail, &[tail.len()]);
    assert!(
        outcome.bytes == [&stripped[..], CSI_CLOSING].concat(),
        "the stripped run, then one DEL"
    );
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
}

/// AC-6 (SPEC AC-6; FR3, NFR3), regression pin: a run ending in plain bytes is
/// written whole with `pending` empty.
#[test]
fn overflow_lone_esc_a_run_ending_in_plain_bytes_flushes_whole() {
    let held = osc_held_at_the_cap();
    let tail = ten_bel_abc();
    let (filter, outcome, stripped) = flush_after_held(&held, &tail, &[]);
    assert!(outcome.bytes == stripped);
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
}

// ── AC-7 (FR1, FR6, TM-1): the production reader ─────────────────────────

/// AC-7 (SPEC AC-7; FR1, FR6, TM-1): reads that grow an `ESC]0;` OSC past the
/// cap, the read that crosses it ending in BEL, `abc` and `ESC`, then a read of
/// `[6n`, through the production reader (`run_reader_without_owner`). The pane's
/// ring holds no `ESC[6n` and term_core replaying it reports no cursor position.
#[test]
fn overflow_lone_esc_the_production_reader_leaves_no_query_in_the_ring() {
    let start = std::time::Instant::now();
    let mut chunks: Vec<Vec<u8>> = vec![b"\x1b]0;".to_vec()];
    chunks.extend(std::iter::repeat_n(vec![b'p'; 60_000], 8));
    // The read that crosses the cap: more body bytes, BEL, text and an ESC.
    let mut crossing = vec![b'p'; 50_000];
    crossing.extend_from_slice(BEL);
    crossing.extend_from_slice(b"abc");
    crossing.extend_from_slice(ESC);
    chunks.push(crossing);
    chunks.push(b"[6n".to_vec());

    let mut expected = b"\x1b]0;".to_vec();
    expected.resize(expected.len() + 8 * 60_000 + 50_000, b'p');
    expected.extend_from_slice(BEL);
    expected.extend_from_slice(b"abc");

    let ring = run_reader_without_owner(chunks);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert!(
        !contains_bytes(&ring, b"\x1b[6n"),
        "the ring holds no ESC[6n"
    );
    assert!(
        ring == expected,
        "the ring holds the stripped run and neither the held ESC nor the query"
    );
    assert!(
        view_of(&ring).responses.is_empty(),
        "term_core replaying the ring reports no cursor position"
    );
}
