//! mux-write-filter-overflow-open-string-cut task0001 (FR1-FR7, NFR1-NFR5): the
//! written end state of the scrollback write filter distinguishes an open OSC
//! body and an open ST-terminated (DCS / APC) body from ground, and every cut
//! that finds the written stream in one of them writes `ESC` + CAN.
//!
//! The overflow flush writes a string body the strip never closed. The written
//! end state used to count such a body as ground, so a later cut (a removed
//! 47 / 1047 / 1049 switch, or the reader's fallback closing) wrote no closure:
//! the client's parser closed the string at the switch's `ESC`, but a ring replay
//! stayed inside it and absorbed the plain text written after the cut (a later
//! BEL would then complete the string with that text). The closure is now the
//! string-body closure `ESC` + CAN, chosen from the written end state alone on
//! every cut path.
//!
//! Oracle convention (the one overflow_lone_esc uses): replay checks compare
//! term_core views (rows, cursor, responses); for a cut the reference is
//! term_core fed the raw stream with a 47 / 1047 / 1049 `h` / `l` pair in place
//! of the cut, the removed construct's own effect kept out (`view_after_a_cut`).
//! Exact-bytes checks compare against the write-path strip. A held run is about
//! 0.5 MiB: each test builds it once and keeps the number of passes over it
//! small (the replay of a run does not depend on the switch pair, so it is
//! computed once and compared with the reference of every pair).
//!
//! Test identifiers: AC-2 the held-`ESC` ending, AC-3 the plain-body ending and
//! the flush followed by a cut in the same call, AC-4 the DCS / APC bodies, AC-5
//! the state carried across calls, AC-7 the production reader, AC-8 the closure,
//! AC-11 the non-overflow strip-then-cut case. AC-1 lives in
//! `scrollback_filter::tests`, AC-6 in `overflow_lone_esc` and
//! `round4_designator_cut`.

use super::post_strip_cut_csi::{BUDGET, KITTY, all_targets};
use super::round3_write_path::{DIMS, run_reader_without_owner, switch_pairs};
use super::round4_cut_csi::{View, osc_held_at_the_cap, text, view_after_a_cut, view_of};
use super::*;
use crate::mux::scrollback_filter::tests::{client_written_state, csi};
use crate::mux::scrollback_filter::{
    strip_pty_output_for_scrollback_write, strip_replayable_rich_content,
};

const ESC: &[u8] = &[0x1b];
const BEL: &[u8] = &[0x07];

/// The plain text written after a cut: it must be displayed on replay and never
/// absorbed into a string body.
pub(super) const AFTER: &[u8] = b"after the cut";

/// The ten body bytes an overflowing call adds to the held OSC.
const TEN_BODY_BYTES: &[u8] = b"pppppppppp";

// ── helpers ──────────────────────────────────────────────────────────────

/// Whether the view shows [`AFTER`] on a row.
pub(super) fn shows_after_text(view: &View) -> bool {
    let shown = String::from_utf8_lossy(AFTER).into_owned();
    view.rows.iter().any(|row| row.contains(&shown))
}

/// A fresh filter fed `held` (the string held at the cap: nothing is written) and
/// then `call1` in one cut-free call, which takes the overflow flush.
pub(super) fn overflow(held: &[u8], call1: &[u8]) -> (ScrollbackWriteFilter, FeedOutcome) {
    let mut filter = ScrollbackWriteFilter::new();
    assert!(
        filter.feed(held, DIMS).1.is_empty(),
        "the string held at the cap is still held"
    );
    assert_eq!(filter.pending_len(), SCROLLBACK_FILTER_PENDING_CAP);
    let outcome = filter.feed_with_cuts(call1, DIMS, &[]);
    assert!(outcome.carried.is_none(), "the flushed call reports none");
    (filter, outcome)
}

/// The write-path strip of `parts` joined.
pub(super) fn strip_of(parts: &[&[u8]]) -> Vec<u8> {
    strip_pty_output_for_scrollback_write(&parts.concat())
}

/// The reference views of the raw stream `raw_before` followed by a removed
/// 47 / 1047 / 1049 `h` / `l` pair and [`AFTER`], one per pair. The raw stream
/// gives no response to the text, and shows it.
fn references_after(raw_before: &[u8]) -> Vec<(String, View)> {
    switch_pairs()
        .into_iter()
        .map(|(enter, leave)| {
            let reference = view_after_a_cut(&[raw_before, enter, leave].concat(), AFTER);
            assert!(
                reference.responses.is_empty(),
                "the raw stream gives no response"
            );
            assert!(
                shows_after_text(&reference),
                "the raw stream shows the text after the switch"
            );
            (text(enter), reference)
        })
        .collect()
}

/// Assert that term_core replaying `written` and then `later` shows the text and
/// equals the reference of every switch pair.
#[track_caller]
fn assert_replay_matches(references: &[(String, View)], written: &[u8], later: &[u8], ctx: &str) {
    let replay = view_after_a_cut(written, later);
    assert!(
        shows_after_text(&replay),
        "{ctx}: the text after the cut is displayed"
    );
    for (switch, reference) in references {
        assert_eq!(
            &replay, reference,
            "{ctx}, switch {switch}: the replay matches the raw stream with the switch for the cut"
        );
    }
}

/// One string body that can be held at the cap.
pub(super) struct Body {
    pub(super) name: &'static str,
    /// The introducer and the first bytes of the body.
    pub(super) head: &'static [u8],
    /// The filler byte of the rest of the body.
    pub(super) pad: u8,
}

impl Body {
    /// The run of this string that fills the pending buffer exactly to the cap
    /// (still held: only a run past the cap is flushed).
    pub(super) fn held_at_the_cap(&self) -> Vec<u8> {
        let mut held = self.head.to_vec();
        held.resize(SCROLLBACK_FILTER_PENDING_CAP, self.pad);
        held
    }

    /// Ten more body bytes: they take the held run past the cap.
    pub(super) fn ten_more(&self) -> Vec<u8> {
        vec![self.pad; 10]
    }
}

/// The DCS / APC bodies of AC-4: a non-strip-target DCS, a non-strip-target APC,
/// a Kitty APC and a SIXEL DCS. The Kitty and SIXEL payloads neither answer nor
/// place an image, so the view comparison is not dominated by image side effects.
pub(super) fn st_bodies() -> [Body; 4] {
    [
        Body {
            name: "non-strip-target dcs",
            head: b"\x1bPp",
            pad: b'p',
        },
        Body {
            name: "non-strip-target apc",
            head: b"\x1b_x",
            pad: b'p',
        },
        Body {
            name: "kitty apc",
            head: b"\x1b_Gi=1,a=d;",
            pad: b'A',
        },
        Body {
            name: "sixel dcs",
            head: b"\x1bPq#0;2;0;0;0",
            pad: b';',
        },
    ]
}

// ── AC-2 (FR2, FR4, FR5, TM-1): the held-ESC open-body ending ────────────

/// AC-2 (SPEC AC-2; FR2, FR4, FR5, TM-1): the OSC held at the cap and call 1 of
/// body bytes and a final `ESC`: exactly one `ESC` is held and the written state
/// is the open OSC body. The reader's fallback closing writes exactly `ESC` + CAN,
/// leaves `pending` empty, the written state Ground and no designator awaited, and
/// a second closing writes nothing; the replay of the written bytes plus a later
/// plain-text call equals the raw stream with the switch for the cut, for each
/// 47 / 1047 / 1049 pair, and the text is displayed.
#[test]
fn overflow_open_string_cut_the_fallback_closing_after_a_held_esc_writes_esc_can() {
    let start = std::time::Instant::now();
    let held = osc_held_at_the_cap();
    let call1 = [TEN_BODY_BYTES, ESC].concat();

    let (mut filter, outcome) = overflow(&held, &call1);
    assert_eq!(filter.pending(), ESC, "exactly the one ESC is held");
    assert_eq!(
        filter.written_state(),
        WrittenState::OscBody,
        "the written state is the open OSC body"
    );
    assert!(
        outcome.bytes == strip_of(&[&held[..], TEN_BODY_BYTES]),
        "the final ESC is not written"
    );

    let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
    assert_eq!(closing, STRING_BODY_CLOSING.to_vec(), "exactly ESC and CAN");
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
    assert!(
        filter.feed_with_cuts(b"", DIMS, &[0]).bytes.is_empty(),
        "a second closing writes nothing"
    );
    let later = filter.feed(AFTER, DIMS).1;
    assert_eq!(later, AFTER.to_vec(), "the text is written as text");

    let references = references_after(&[&held[..], &call1[..]].concat());
    assert_replay_matches(
        &references,
        &[&outcome.bytes[..], &closing[..]].concat(),
        &later,
        "fallback closing",
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-2 (SPEC AC-3; FR2, FR5, TM-1), separate run: a call carrying a cut at fed 0
/// followed by plain text writes `ESC` + CAN and then the text, with `pending`
/// empty and the state Ground; the replay equals the raw stream with the switch
/// for the cut, for each pair.
#[test]
fn overflow_open_string_cut_a_cut_at_fed_zero_after_a_held_esc_writes_esc_can_then_the_text() {
    let start = std::time::Instant::now();
    let held = osc_held_at_the_cap();
    let call1 = [TEN_BODY_BYTES, ESC].concat();

    let (mut filter, outcome) = overflow(&held, &call1);
    let second = filter.feed_with_cuts(AFTER, DIMS, &[0]);
    assert_eq!(
        second.bytes,
        [STRING_BODY_CLOSING, AFTER].concat(),
        "ESC and CAN, then the text"
    );
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());

    let references = references_after(&[&held[..], &call1[..]].concat());
    assert_replay_matches(&references, &outcome.bytes, &second.bytes, "cut at fed 0");
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-3 (FR2, FR3, FR5, FR6, TM-1): the plain-body ending ───────────────

/// AC-3 (SPEC AC-4; FR2, FR3, TM-1): an overflow run that ends in plain OSC body
/// bytes (no trailing `ESC`) holds nothing and leaves the open OSC body state. A
/// cut at fed 0 of the next call and the fallback closing each write `ESC` + CAN,
/// and the replay with later plain text equals the raw stream with the switch for
/// the cut, for each pair.
#[test]
fn overflow_open_string_cut_a_plain_body_ending_is_closed_by_a_cut_and_by_the_fallback() {
    let start = std::time::Instant::now();
    let held = osc_held_at_the_cap();
    let expected_flush = strip_of(&[&held[..], TEN_BODY_BYTES]);

    // The cut at fed 0 of the next call.
    let (mut filter, outcome) = overflow(&held, TEN_BODY_BYTES);
    assert!(filter.pending().is_empty(), "nothing is held");
    assert_eq!(filter.written_state(), WrittenState::OscBody);
    assert!(outcome.bytes == expected_flush, "the run is written whole");
    let second = filter.feed_with_cuts(AFTER, DIMS, &[0]);
    assert_eq!(
        second.bytes,
        [STRING_BODY_CLOSING, AFTER].concat(),
        "cut at fed 0"
    );
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());

    // The reader's fallback closing.
    let (mut filter, outcome_fallback) = overflow(&held, TEN_BODY_BYTES);
    assert!(
        outcome_fallback.bytes == expected_flush,
        "the same flush on the second run"
    );
    let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
    assert_eq!(closing, STRING_BODY_CLOSING.to_vec(), "fallback closing");
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
    let later = filter.feed(AFTER, DIMS).1;
    assert_eq!(later, AFTER.to_vec());

    // Both paths write the same bytes after the flush, so one replay covers both.
    assert_eq!(
        [&closing[..], &later[..]].concat(),
        second.bytes,
        "the fallback closing and the cut at fed 0 write the same bytes"
    );
    let references = references_after(&[&held[..], TEN_BODY_BYTES].concat());
    assert_replay_matches(&references, &outcome.bytes, &second.bytes, "plain body");
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-3 (SPEC AC-5; FR2, FR5, FR6, TM-1): an overflow flush followed by a cut in
/// the same call, the run ending inside the still-open OSC body. The input is the
/// one the predecessor's third case in
/// `round4_overflow_flush_then_a_cut_without_a_wait_writes_nothing_extra` held:
/// `ESC ]0;` fed first and held, then a call of plain bytes longer than the cap
/// with a cut at its end. The written bytes equal the write-path strip of that run
/// followed by `ESC` + CAN, nothing is held, no designator is awaited, the state is
/// Ground, and the replay equals the raw stream with the switch for the cut.
#[test]
fn overflow_open_string_cut_a_flush_then_a_cut_in_the_same_call_closes_the_open_osc_body() {
    let start = std::time::Instant::now();
    let intro: &[u8] = b"\x1b]0;";
    let pad = vec![b'p'; SCROLLBACK_FILTER_PENDING_CAP + 1];

    let mut filter = ScrollbackWriteFilter::new();
    filter.feed(intro, DIMS);
    let outcome = filter.feed_with_cuts(&pad, DIMS, &[pad.len()]);
    let expected = [&strip_of(&[intro, &pad[..]])[..], STRING_BODY_CLOSING].concat();
    assert!(
        outcome.bytes == expected,
        "the strip of the run, then ESC and CAN"
    );
    assert_eq!(outcome.bytes.len(), intro.len() + pad.len() + 2);
    assert!(filter.pending().is_empty());
    assert!(!filter.awaiting_designator());
    assert_eq!(filter.written_state(), WrittenState::Ground);

    let references = references_after(&[intro, &pad[..]].concat());
    assert_replay_matches(&references, &outcome.bytes, AFTER, "same-call cut");
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-4 (FR1, FR2, TM-1): DCS and APC bodies ────────────────────────────

/// One DCS / APC body of AC-4 (SPEC AC-6; FR1, FR2, TM-1): held at the cap and
/// flushed open past it, the written state is the open ST-terminated body; a cut
/// at fed 0 of the next call and the fallback closing each write `ESC` + CAN and
/// the replay equals the raw stream with the switch for the cut, for each pair.
fn check_an_st_body_left_open_past_the_cap(body: &Body) {
    let start = std::time::Instant::now();
    let name = body.name;
    let held = body.held_at_the_cap();
    let more = body.ten_more();
    let expected_flush = strip_of(&[&held[..], &more[..]]);

    // The cut at fed 0 of the next call.
    let (mut filter, outcome) = overflow(&held, &more);
    assert!(filter.pending().is_empty(), "{name}: nothing is held");
    assert_eq!(
        filter.written_state(),
        WrittenState::StBody,
        "{name}: the open ST-terminated body"
    );
    assert!(
        outcome.bytes == expected_flush,
        "{name}: the run is written whole"
    );
    let second = filter.feed_with_cuts(AFTER, DIMS, &[0]);
    assert_eq!(
        second.bytes,
        [STRING_BODY_CLOSING, AFTER].concat(),
        "{name}: cut at fed 0"
    );
    assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
    assert!(!filter.awaiting_designator(), "{name}");

    // The reader's fallback closing.
    let (mut filter, outcome_fallback) = overflow(&held, &more);
    assert!(outcome_fallback.bytes == expected_flush, "{name}");
    assert_eq!(
        filter.written_state(),
        WrittenState::StBody,
        "{name}: the open ST-terminated body"
    );
    let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
    assert_eq!(
        closing,
        STRING_BODY_CLOSING.to_vec(),
        "{name}: fallback closing"
    );
    assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
    let later = filter.feed(AFTER, DIMS).1;
    assert_eq!(
        [&closing[..], &later[..]].concat(),
        second.bytes,
        "{name}: the fallback closing and the cut at fed 0 write the same bytes"
    );

    let references = references_after(&[&held[..], &more[..]].concat());
    assert_replay_matches(&references, &outcome.bytes, &second.bytes, name);
    assert!(
        start.elapsed() < BUDGET,
        "{name}: took {:?}",
        start.elapsed()
    );
}

#[test]
fn overflow_open_string_cut_a_non_strip_target_dcs_left_open_past_the_cap_is_closed_at_a_cut() {
    check_an_st_body_left_open_past_the_cap(&st_bodies()[0]);
}

#[test]
fn overflow_open_string_cut_a_non_strip_target_apc_left_open_past_the_cap_is_closed_at_a_cut() {
    check_an_st_body_left_open_past_the_cap(&st_bodies()[1]);
}

#[test]
fn overflow_open_string_cut_a_kitty_apc_left_open_past_the_cap_is_closed_at_a_cut() {
    check_an_st_body_left_open_past_the_cap(&st_bodies()[2]);
}

#[test]
fn overflow_open_string_cut_a_sixel_dcs_left_open_past_the_cap_is_closed_at_a_cut() {
    check_an_st_body_left_open_past_the_cap(&st_bodies()[3]);
}

/// AC-4 (SPEC AC-6; FR1): a later call that writes a BEL (and plain bytes) inside
/// a DCS / APC body keeps the ST-terminated body state, and a following cut still
/// writes `ESC` + CAN.
#[test]
fn overflow_open_string_cut_a_bel_inside_a_dcs_or_apc_body_does_not_close_it() {
    let start = std::time::Instant::now();
    for body in st_bodies() {
        let held = body.held_at_the_cap();
        let (mut filter, _outcome) = overflow(&held, &body.ten_more());
        assert_eq!(
            filter.written_state(),
            WrittenState::StBody,
            "{}",
            body.name
        );
        let later_call = [b"abc".as_slice(), BEL, b"def"].concat();
        let later = filter.feed(&later_call, DIMS).1;
        assert_eq!(later, later_call, "{}: written whole", body.name);
        assert_eq!(
            filter.written_state(),
            WrittenState::StBody,
            "{}: the BEL does not close the body",
            body.name
        );
        assert_eq!(
            filter.feed_with_cuts(b"", DIMS, &[0]).bytes,
            STRING_BODY_CLOSING.to_vec(),
            "{}: a following cut still writes ESC and CAN",
            body.name
        );
        assert_eq!(
            filter.written_state(),
            WrittenState::Ground,
            "{}",
            body.name
        );
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-5 (FR3, TS-5): the open-body state carried across calls ───────────

/// AC-5 (SPEC AC-7; FR3): after an overflow leaves an open OSC body, a later call
/// of plain bytes keeps the body state and a following cut writes `ESC` + CAN; a
/// later call that writes a BEL, and one that writes `ESC \`, each return the
/// state to Ground and a following cut writes nothing; a later call that writes an
/// `ESC` followed by `[` leaves the CSI state and a following cut writes DEL.
#[test]
fn overflow_open_string_cut_the_open_osc_body_state_is_carried_and_advanced_by_later_calls() {
    let start = std::time::Instant::now();
    let held = osc_held_at_the_cap();
    let cases: &[(&str, &[u8], WrittenState, &[u8])] = &[
        (
            "plain bytes",
            b"abc",
            WrittenState::OscBody,
            STRING_BODY_CLOSING,
        ),
        ("a BEL", b"x\x07", WrittenState::Ground, b""),
        ("ESC \\", b"x\x1b\\", WrittenState::Ground, b""),
        ("ESC [", b"x\x1b[", csi(CsiPhase::Entry), CSI_CLOSING),
    ];
    for (name, call, state, closing) in cases {
        let (mut filter, _outcome) = overflow(&held, TEN_BODY_BYTES);
        assert_eq!(filter.written_state(), WrittenState::OscBody, "{name}");
        let written = filter.feed(call, DIMS).1;
        assert_eq!(written, call.to_vec(), "{name}: the call is written whole");
        assert_eq!(filter.written_state(), *state, "{name}: the carried state");
        assert!(filter.pending().is_empty(), "{name}");
        assert_eq!(
            filter.feed_with_cuts(b"", DIMS, &[0]).bytes,
            closing.to_vec(),
            "{name}: a following cut writes the closure of that state"
        );
        assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-7 (FR2, FR6, TM-1): the production reader ─────────────────────────

/// AC-7 (SPEC AC-1; FR2, FR6, TM-1), one switch pair: through the production
/// reader (`run_reader_without_owner`), reads grow an `ESC]0;` OSC past the cap,
/// the read that crosses the cap ends in `ESC`, and the next read begins with a
/// 47 / 1047 / 1049 `h` ... `l` pair followed by main-buffer plain text. The ring
/// holds the string-body closure once, term_core replaying the ring displays that
/// text, and its view (rows, cursor, responses) equals the reference (term_core
/// fed the raw stream).
fn check_the_production_reader_closes_the_overflowed_osc(pair: usize) {
    let start = std::time::Instant::now();
    let (enter, leave) = switch_pairs()[pair];
    let ctx = format!("switch {}", text(enter));
    let mut chunks: Vec<Vec<u8>> = vec![b"\x1b]0;".to_vec()];
    chunks.extend(std::iter::repeat_n(vec![b'p'; 60_000], 8));
    // The read that crosses the cap: more body bytes and a final ESC.
    let mut crossing = vec![b'p'; 50_000];
    crossing.extend_from_slice(ESC);
    chunks.push(crossing);
    // The next read: the switch pair, then main-buffer plain text.
    chunks.push([enter, leave, AFTER].concat());

    let mut expected = b"\x1b]0;".to_vec();
    expected.resize(expected.len() + 8 * 60_000 + 50_000, b'p');
    expected.extend_from_slice(STRING_BODY_CLOSING);
    expected.extend_from_slice(AFTER);

    let raw: Vec<u8> = chunks.concat();
    let ring = run_reader_without_owner(chunks);
    assert!(
        ring == expected,
        "{ctx}: the body, the closure once, the text"
    );
    let closures = ring
        .windows(STRING_BODY_CLOSING.len())
        .filter(|window| *window == STRING_BODY_CLOSING)
        .count();
    assert_eq!(
        closures, 1,
        "{ctx}: the string-body closure is in the ring once"
    );

    let replay = view_of(&ring);
    assert!(
        shows_after_text(&replay),
        "{ctx}: term_core replaying the ring displays the text"
    );
    assert_eq!(
        replay,
        view_of(&raw),
        "{ctx}: the replay equals term_core fed the raw stream"
    );
    assert!(
        start.elapsed() < BUDGET,
        "{ctx}: took {:?}",
        start.elapsed()
    );
}

#[test]
fn overflow_open_string_cut_the_production_reader_closes_the_overflowed_osc_at_switch_47() {
    check_the_production_reader_closes_the_overflowed_osc(0);
}

#[test]
fn overflow_open_string_cut_the_production_reader_closes_the_overflowed_osc_at_switch_1047() {
    check_the_production_reader_closes_the_overflowed_osc(1);
}

#[test]
fn overflow_open_string_cut_the_production_reader_closes_the_overflowed_osc_at_switch_1049() {
    check_the_production_reader_closes_the_overflowed_osc(2);
}

// ── AC-8 (FR5, TM-2): the string-body closure ────────────────────────────

/// AC-8 (SPEC FR5; TM-2): the string-body closure is the two bytes `ESC`, CAN,
/// distinct from the other three closures.
#[test]
fn overflow_open_string_cut_the_string_body_closure_is_esc_then_can_and_distinct() {
    assert_eq!(STRING_BODY_CLOSING, &[0x1b, 0x18][..]);
    assert_eq!(
        STRING_BODY_CLOSING,
        [ESC, ESCAPE_CLOSING].concat().as_slice()
    );
    assert_ne!(STRING_BODY_CLOSING, CSI_CLOSING);
    assert_ne!(STRING_BODY_CLOSING, ESCAPE_CLOSING);
    assert_ne!(STRING_BODY_CLOSING, ESC, "not the designator closure");
}

/// Streams that end inside an open OSC, DCS and APC body whose completion or
/// abort answers nothing.
const OPEN_BODY_STREAMS: &[(&str, &[u8])] = &[
    ("osc", b"before\x1b]0;title"),
    ("dcs", b"before\x1bPpayload"),
    ("apc", b"before\x1b_payload"),
    ("kitty apc", b"before\x1b_Gi=1,a=d;AAAA"),
    ("sixel dcs", b"before\x1bPq#0;2;0;0;0"),
];

/// AC-8 (SPEC FR5, FR2; TM-2): for streams ending inside an open OSC, DCS or APC
/// body, the stream followed by the closure ends in Ground per the end-state
/// oracle, and probe text after it (including `[6n`, `\` and BEL-terminated text)
/// replays as the stream closed by a switch pair, with no response.
#[test]
fn overflow_open_string_cut_the_closure_returns_every_open_body_to_ground() {
    let probes: &[&[u8]] = &[b"[6n", b"\\", b"title\x07", b"plain", b"\x07", b"]0;t\x07"];
    for (name, stream) in OPEN_BODY_STREAMS {
        let closed = [*stream, STRING_BODY_CLOSING].concat();
        assert_eq!(
            client_written_state(stream),
            match *name {
                "osc" => WrittenState::OscBody,
                _ => WrittenState::StBody,
            },
            "{name}: the stream ends inside the open body"
        );
        assert_eq!(
            client_written_state(&closed),
            WrittenState::Ground,
            "{name}: the stream and the closure end in Ground"
        );
        for probe in probes {
            for (enter, leave) in switch_pairs() {
                let ctx = format!("{name}, switch {}, probe {:?}", text(enter), text(probe));
                let reference = view_after_a_cut(&[*stream, enter, leave].concat(), probe);
                let replay = view_after_a_cut(&closed, probe);
                assert_eq!(replay, reference, "{ctx}");
                assert!(replay.responses.is_empty(), "{ctx}: no response");
            }
        }
    }
}

/// What both strips return for an open body stream followed by the closure
/// (mux-snapshot-strip-can-abort FR1-FR4): an open Kitty APC / SIXEL DCS body is
/// aborted by the closure's `ESC` and removed up to it, so the strips return
/// `before` and the closure; every other open body (OSC, non-target DCS / APC) is
/// kept whole.
fn closed_stream_after_the_strip(name: &str, stream: &[u8]) -> Vec<u8> {
    if name.contains("kitty") || name.contains("sixel") {
        [b"before".as_slice(), STRING_BODY_CLOSING].concat()
    } else {
        [stream, STRING_BODY_CLOSING].concat()
    }
}

/// AC-8 (SPEC FR5; TM-2; mux-snapshot-strip-can-abort AC-8a, FR3): neither the
/// write-path strip nor the snapshot-time strip reads the closure as the start of
/// a strip target or as ST, and a strip target written right after the closure is
/// still removed. The closure's `ESC` aborts an open Kitty APC / SIXEL DCS body:
/// both strips return `before` and the closure for those streams, and keep an
/// open OSC, non-target DCS or non-target APC stream and the closure whole.
#[test]
fn overflow_open_string_cut_neither_strip_reads_the_closure_as_a_target_or_as_st() {
    type Strip = fn(&[u8]) -> Vec<u8>;
    let strips: [(&str, Strip); 2] = [
        ("write path", strip_pty_output_for_scrollback_write),
        ("snapshot time", strip_replayable_rich_content),
    ];
    for (strip_name, strip) in strips {
        assert_eq!(
            strip(STRING_BODY_CLOSING),
            STRING_BODY_CLOSING.to_vec(),
            "{strip_name}: the closure alone is kept"
        );
        // An open Kitty APC / SIXEL DCS followed by the closure is no complete
        // target: the closure is not ST. Its `ESC` aborts the body, which is
        // removed up to that `ESC`.
        for (name, stream) in OPEN_BODY_STREAMS {
            let closed = [*stream, STRING_BODY_CLOSING].concat();
            assert_eq!(
                strip(&closed),
                closed_stream_after_the_strip(name, stream),
                "{strip_name}, {name}: the closure forms no ST"
            );
        }
        // A strip target right after the closure is still removed, whichever
        // open body the closure follows.
        for (name, target) in all_targets() {
            for (open_name, stream) in OPEN_BODY_STREAMS {
                let input = [*stream, STRING_BODY_CLOSING, target].concat();
                assert_eq!(
                    strip(&input),
                    closed_stream_after_the_strip(open_name, stream),
                    "{strip_name}: {name} after the closure of {open_name}"
                );
            }
        }
    }
    // The Kitty target is a complete target when closed by its own ST.
    assert!(strip_pty_output_for_scrollback_write(KITTY).is_empty());
}

// ── AC-11 (FR1, FR2, NFR3, TM-1): a non-overflow strip-then-cut ──────────

/// The call of AC-11: an OSC and an answered device query that aborts it. The
/// boundary scan holds nothing, and the strip removes the query.
const OSC_THEN_QUERY: &[u8] = b"\x1b]0;x\x1b[6n";

/// AC-11 (IMPLEMENTATION.md D3; FR1, FR2, NFR3, TM-1): a call far below the cap
/// fed `ESC ]0;x` followed by `ESC [6n` holds nothing and writes the write-path
/// strip of its input, `ESC ]0;x`; the written state is the open OSC body, as the
/// end-state oracle classifies those written bytes. A next call carrying a cut at
/// fed 0 followed by plain text writes `ESC` + CAN and then the text, with
/// `pending` empty, the state Ground and no designator awaited. In a separate run,
/// the reader's fallback closing in place of that call writes exactly `ESC` + CAN.
/// For each pair, the replay of each run's written bytes plus a later plain-text
/// call equals the raw stream with the switch for the cut (the query's answer
/// kept out), and the text is displayed.
#[test]
fn overflow_open_string_cut_a_non_overflow_strip_then_cut_closes_the_open_osc_body() {
    let references = references_after(OSC_THEN_QUERY);

    // The cut at fed 0 of the next call.
    let mut filter = ScrollbackWriteFilter::new();
    let first = filter.feed_with_cuts(OSC_THEN_QUERY, DIMS, &[]);
    assert!(filter.pending().is_empty(), "nothing is held");
    assert_eq!(
        first.bytes,
        strip_of(&[OSC_THEN_QUERY]),
        "the write-path strip of the input"
    );
    assert_eq!(first.bytes, b"\x1b]0;x".to_vec(), "the query is removed");
    assert_eq!(filter.written_state(), WrittenState::OscBody);
    assert_eq!(
        filter.written_state(),
        client_written_state(&first.bytes),
        "term_core classifies the written bytes as the open OSC body"
    );
    let second = filter.feed_with_cuts(AFTER, DIMS, &[0]);
    assert_eq!(
        second.bytes,
        [STRING_BODY_CLOSING, AFTER].concat(),
        "ESC and CAN, then the text"
    );
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
    assert_replay_matches(&references, &first.bytes, &second.bytes, "cut at fed 0");

    // The reader's fallback closing in place of that call.
    let mut filter = ScrollbackWriteFilter::new();
    let first = filter.feed_with_cuts(OSC_THEN_QUERY, DIMS, &[]);
    let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
    assert_eq!(closing, STRING_BODY_CLOSING.to_vec(), "fallback closing");
    assert!(filter.pending().is_empty());
    assert_eq!(filter.written_state(), WrittenState::Ground);
    assert!(!filter.awaiting_designator());
    let later = filter.feed(AFTER, DIMS).1;
    assert_eq!(later, AFTER.to_vec());
    assert_replay_matches(
        &references,
        &[&first.bytes[..], &closing[..]].concat(),
        &later,
        "fallback closing",
    );
}

// ── AC-10 (FR7, TS-8): the doc comments state the open-body model ────────

/// The comment lines (`//`, `///`, `//!`) of a source file with their markers
/// removed, joined by single spaces: the text the doc-comment contract below
/// reads, so a phrase wrapped over two lines is still one phrase.
pub(super) fn comment_text(source: &str) -> String {
    source
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("//"))
        .map(|line| line.trim_start_matches(['/', '!']))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

/// AC-10 (SPEC AC-10; FR7, TS-8): the comments FR7 lists (the WrittenState doc,
/// the escape-state comment in its advance, the closure_for and ESCAPE_CLOSING
/// docs, the "Overflow" and "Closure at a cut" paragraphs of feed_with_cuts, the
/// overflow-branch comments, the written-state field doc, the cut-branch comments)
/// state the open-body states and the `ESC` + CAN closure; none states that a
/// string body counts as ground, that a string body needs no state, or that a cut
/// writes at most one of two closures.
#[test]
fn overflow_open_string_cut_the_doc_comments_state_the_open_body_model() {
    let strip = comment_text(include_str!("../../../scrollback_filter.rs"));
    let filter = comment_text(include_str!("../write_filter.rs"));

    let stale: &[(&str, &str, &str)] = &[
        ("scrollback_filter.rs", &strip, "needs no state of its own"),
        (
            "scrollback_filter.rs",
            &strip,
            "for this question, is ground",
        ),
        ("write_filter.rs", &filter, "counts a string body as ground"),
        ("write_filter.rs", &filter, "a string body counts as"),
        ("write_filter.rs", &filter, "at most one of the two"),
        ("write_filter.rs", &filter, "writes at most one closure"),
    ];
    for (file, comments, phrase) in stale {
        assert!(
            !comments.contains(phrase),
            "{file}: a comment still says {phrase:?}"
        );
    }

    let required: &[(&str, &str, &str)] = &[
        ("scrollback_filter.rs", &strip, "OSC body"),
        ("scrollback_filter.rs", &strip, "DCS / APC body"),
        ("write_filter.rs", &filter, "STRING_BODY_CLOSING"),
        ("write_filter.rs", &filter, "open string body"),
        ("write_filter.rs", &filter, "`ESC` + CAN"),
    ];
    for (file, comments, phrase) in required {
        assert!(
            comments.contains(phrase),
            "{file}: no comment says {phrase:?}"
        );
    }
}
