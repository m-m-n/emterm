//! mux-strip-open-string-body-closure task0001 (FR1-FR10, NFR1-NFR3): a construct
//! the shared strip removes while the written stream is inside an open OSC body
//! or an open ST-terminated (DCS / APC) body is replaced by the string-body
//! closure, `ESC` then CAN, so the bytes written after the removal can never join
//! the body.
//!
//! Before, such a removal wrote nothing and the body stayed open: `ESC]11;` + a
//! removed construct + `?` BEL was written as `ESC]11;?` BEL, which term_core
//! takes as a complete OSC 11 color query, while the raw stream aborted the OSC at
//! the construct's `ESC`. The ring then made the client answer a query the raw
//! stream never made, on a snapshot replay and for a live `?` BEL that follows a
//! ring ending in `ESC]11;` + a removed construct.
//!
//! The strip-level cases (every entry point, the carried state form, the remap
//! and the linear pass) live in `scrollback_filter::tests`, and the segment
//! mapping of the snapshot builder in `snapshot_bytes::tests`. The write filter,
//! the snapshot replay, the live continuation, the cuts and the doc contracts are
//! checked here.
//!
//! Oracle convention (NFR3): the reference is term_core fed the raw stream, with
//! the removed construct's own effect kept out (`view_after_a_cut`), comparing
//! rows, cursor and responses. Every "no color response" assertion goes through
//! the themed client (`themed_client`, the client the GUI builds: it answers OSC
//! 4 / 10 / 11 / 12 queries) and has a control that shows the themed client
//! answers; a client without that responder answers no color query, so such an
//! assertion would pass vacuously. The themed-client tests are compiled only with
//! the `gui` feature, like the helper.
//!
//! Test identifiers: AC-2 (b) to (d) the carried and split writes, AC-3 the
//! snapshot replay, AC-4 the live continuation, AC-6 no double closure and an
//! inert closure, AC-7 the single definition and the doc comments, AC-8 and AC-9
//! the records. AC-1, AC-2 (a), AC-5 and the linear pass of AC-10 live in
//! `scrollback_filter::tests` and `snapshot_bytes::tests`.

use super::overflow_open_string_cut::{AFTER, TEN_BODY_BYTES, comment_text, overflow, st_bodies};
use super::post_strip_cut_csi::BUDGET;
use super::round3_write_path::DIMS;
#[cfg(feature = "gui")]
use super::round4_cut_csi::View;
use super::round4_cut_csi::{osc_held_at_the_cap, text, view_after_a_cut, view_of};
use super::strip_concat_query::{TARGETS, Target};
use super::*;
use crate::mux::scrollback_filter::{
    strip_pty_output_for_scrollback_write, strip_replayable_rich_content, vt100_replay_copy,
};
#[cfg(feature = "gui")]
use crate::mux::snapshot_bytes::build_resume_snapshot_bytes;
use crate::mux::snapshot_bytes::build_snapshot_bytes;
use std::time::Instant;

const ESC: &[u8] = &[0x1b];

/// What the build step puts before and after the stripped scrollback of a
/// main-buffer pane in the reattach layout, and before it in the resume layout.
const SNAPSHOT_HEAD: &[u8] = b"\x1b[3J\x1b[H\x1b[2J";
const SNAPSHOT_TAIL: &[u8] = b"\x1b[?1049l";
#[cfg(feature = "gui")]
const RESUME_CLEAR: &[u8] = b"\x1b[H\x1b[2J";

/// `?` followed by BEL: the bytes that complete an OSC color query when they join
/// an open color head.
const QUERY_TAIL: &[u8] = b"?\x07";

/// The open string bodies of the plan: an OSC title, an OSC color head, a DCS and
/// a non-Kitty APC. Their abort answers nothing, and none is a strip target.
const BODY_HEADS: &[&[u8]] = &[b"\x1b]0;t", b"\x1b]11;", b"\x1bPx", b"\x1b_Xnot-kitty"];

/// The OSC color heads: 11 (background), 10 (foreground), 12 (cursor color) and 4
/// (palette entry 1). Each answers once a `?` and a BEL join it.
const COLOR_HEADS: &[&[u8]] = &[b"\x1b]11;", b"\x1b]10;", b"\x1b]12;", b"\x1b]4;1;"];

/// The DCS and APC heads whose abort answers nothing.
#[cfg(feature = "gui")]
const ST_HEADS: &[&[u8]] = &[b"\x1bPx", b"\x1b_x"];

/// What follows the removed construct in the many-case loops.
const CONTINUATIONS: &[&[u8]] = &[b"n", QUERY_TAIL, b"text", b"\x1b\\"];

// ── helpers ──────────────────────────────────────────────────────────────

/// The write filter's output for `fed`, in one cut-free call to a fresh filter,
/// which holds nothing afterwards and leaves Ground.
#[track_caller]
fn write_once(fed: &[u8], ctx: &str) -> Vec<u8> {
    let mut filter = ScrollbackWriteFilter::new();
    let out = filter.feed(fed, DIMS).1;
    assert!(
        filter.pending().is_empty(),
        "{ctx}: the filter holds {:?}",
        text(filter.pending())
    );
    assert!(!filter.awaiting_designator(), "{ctx}");
    assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
    out
}

/// The ring a head, a removed construct and a continuation leave: the head, the
/// string-body closure, the construct's C0 bytes and the continuation.
fn closed(head: &[u8], target: &Target, cont: &[u8]) -> Vec<u8> {
    [head, STRING_BODY_CLOSING, target.c0, cont].concat()
}

/// The stripped scrollback part of a reattach snapshot of `ring`.
#[track_caller]
fn snapshot_scrollback(ring: &[u8], ctx: &str) -> (Vec<u8>, Vec<u8>) {
    let (payload, _segments) = build_snapshot_bytes(ring, &[], b"", false, DIMS);
    let stripped = payload
        .strip_prefix(SNAPSHOT_HEAD)
        .and_then(|rest| rest.strip_suffix(SNAPSHOT_TAIL))
        .unwrap_or_else(|| panic!("{ctx}: unexpected snapshot layout {:?}", text(&payload)))
        .to_vec();
    (payload, stripped)
}

/// Replaying `ring` gives no response and the rows and cursor of the raw stream
/// `raw_before` followed by `after`, with the answers to `raw_before` (the
/// removed construct's own effect) discarded.
#[track_caller]
fn assert_replays_like_raw(ring: &[u8], raw_before: &[u8], after: &[u8], ctx: &str) {
    let replayed = view_of(ring);
    let reference = view_after_a_cut(raw_before, after);
    assert!(
        replayed.responses.is_empty(),
        "{ctx}: replaying the ring answers {:?}",
        replayed.responses
    );
    assert!(
        reference.responses.is_empty(),
        "{ctx}: the raw stream answers after the construct"
    );
    assert_eq!(replayed.rows, reference.rows, "{ctx}: rows");
    assert_eq!(replayed.cursor, reference.cursor, "{ctx}: cursor");
}

#[cfg(feature = "gui")]
fn themed_view(core: &mut term_core::terminal_core::TerminalCore) -> View {
    let responses = core.take_response();
    View {
        rows: (0..24)
            .map(|r| core.get_line_text(r).trim_end().to_string())
            .collect(),
        cursor: (core.get_cursor_row(), core.get_cursor_col()),
        responses,
    }
}

/// What the themed client shows and answers after `stream`.
#[cfg(feature = "gui")]
fn themed_view_of(stream: &[u8]) -> View {
    let mut core = themed_client(80, 24);
    core.process_pty_data_fully(stream);
    themed_view(&mut core)
}

/// The themed client after `before` and then `after`, with the answers to
/// `before` discarded.
#[cfg(feature = "gui")]
fn themed_view_after_a_cut(before: &[u8], after: &[u8]) -> View {
    let mut core = themed_client(80, 24);
    core.process_pty_data_fully(before);
    let _own_answers = core.take_response();
    core.process_pty_data_fully(after);
    themed_view(&mut core)
}

/// What the themed client shows and answers after replaying a snapshot payload
/// the way the GUI applies it (`reset_and_replay_segments`), with the responses
/// kept so the assertion can see a color answer the GUI would discard.
#[cfg(feature = "gui")]
fn themed_view_of_a_snapshot(payload: &[u8]) -> View {
    let mut core = themed_client(80, 24);
    core.reset_and_replay_segments(payload, &[]);
    themed_view(&mut core)
}

// ── AC-2 (b) (FR1, FR3, NFR3, TM-1): a body an overflow flush left open ──

/// The next call after an overflow flush left `body` open: each removed construct
/// followed by `?` BEL and plain text writes the string-body closure, the
/// construct's C0 bytes, `?` BEL and the text; nothing is held, the state is
/// Ground, and the replay of everything written equals the raw stream.
fn check_a_flushed_open_body_is_closed_at_the_next_removal(
    name: &str,
    held: &[u8],
    more: &[u8],
    open: WrittenState,
) {
    for target in TARGETS {
        let ctx = format!("{name} + {}", target.name);
        let after = [QUERY_TAIL, b"plain text"].concat();
        let call2 = [target.bytes, &after[..]].concat();

        let (mut filter, flush) = overflow(held, more);
        assert_eq!(
            filter.written_state(),
            open,
            "{ctx}: the flush leaves the body open"
        );
        let second = filter.feed_with_cuts(&call2, DIMS, &[]);
        let expected = [STRING_BODY_CLOSING, target.c0, &after[..]].concat();
        assert!(
            second.bytes == expected,
            "{ctx}: wrote {:?}, expected {:?}",
            text(&second.bytes),
            text(&expected)
        );
        assert!(filter.pending().is_empty(), "{ctx}: nothing is held");
        assert!(!filter.awaiting_designator(), "{ctx}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");

        let ring = [&flush.bytes[..], &second.bytes[..]].concat();
        let raw_before = [held, more, target.bytes].concat();
        assert_replays_like_raw(&ring, &raw_before, &after, &ctx);
    }
}

/// AC-2 (b): an OSC body held at the cap and flushed past it.
#[test]
fn open_body_closure_an_osc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal() {
    let start = Instant::now();
    check_a_flushed_open_body_is_closed_at_the_next_removal(
        "osc",
        &osc_held_at_the_cap(),
        TEN_BODY_BYTES,
        WrittenState::OscBody,
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-2 (b): each of the four DCS / APC bodies `overflow_open_string_cut` holds at
/// the cap, flushed past it.
#[test]
fn open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal()
 {
    let start = Instant::now();
    for body in st_bodies() {
        check_a_flushed_open_body_is_closed_at_the_next_removal(
            body.name,
            &body.held_at_the_cap(),
            &body.ten_more(),
            WrittenState::StBody,
        );
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-2 (c) (FR3, TM-1): a held live lone ESC after an overflow ─────────

/// AC-2 (c): an overflow call that ends with body bytes and a live lone `ESC`
/// holds that `ESC` and leaves the written state in the open OSC body. A next
/// call of each construct WITHOUT its `ESC`, then `?` BEL and plain text, joins
/// the held `ESC`, removes the construct and writes the closure first; nothing is
/// held afterwards and the state is Ground.
#[test]
fn open_body_closure_a_held_lone_esc_joins_the_next_construct_and_the_body_is_closed() {
    let start = Instant::now();
    let held = osc_held_at_the_cap();
    let call1 = [TEN_BODY_BYTES, ESC].concat();
    let after = [QUERY_TAIL, b"plain text"].concat();
    for target in TARGETS {
        let ctx = target.name;
        let (mut filter, flush) = overflow(&held, &call1);
        assert_eq!(filter.pending(), ESC, "{ctx}: exactly the one ESC is held");
        assert_eq!(filter.written_state(), WrittenState::OscBody, "{ctx}");

        let call2 = [&target.bytes[1..], &after[..]].concat();
        let second = filter.feed_with_cuts(&call2, DIMS, &[]);
        let expected = [STRING_BODY_CLOSING, target.c0, &after[..]].concat();
        assert!(
            second.bytes == expected,
            "{ctx}: wrote {:?}, expected {:?}",
            text(&second.bytes),
            text(&expected)
        );
        assert!(filter.pending().is_empty(), "{ctx}: nothing is held");
        assert!(!filter.awaiting_designator(), "{ctx}");
        assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");

        let ring = [&flush.bytes[..], &second.bytes[..]].concat();
        let raw_before = [&held[..], TEN_BODY_BYTES, target.bytes].concat();
        assert_replays_like_raw(&ring, &raw_before, &after, ctx);
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-2 (d) (FR3, TM-1): split reads ────────────────────────────────────

/// The write filter's output for `input` fed as `a` then `b`, no cut.
fn write_split(a: &[u8], b: &[u8]) -> (Vec<u8>, ScrollbackWriteFilter) {
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = filter.feed(a, DIMS).1;
    ring.extend_from_slice(&filter.feed(b, DIMS).1);
    (ring, filter)
}

/// AC-2 (d): a body head, each string construct the filter holds across reads and
/// `?` BEL, split into two calls at every position with no cut, writes the same
/// bytes as one call: the head and the closure at the construct, then `?` BEL. No
/// split position lets a later byte join the body.
#[test]
fn open_body_closure_a_split_read_writes_the_same_bytes_as_one_call() {
    let start = Instant::now();
    for head in BODY_HEADS {
        for target in TARGETS.iter().filter(|t| t.held) {
            let input = [*head, target.bytes, QUERY_TAIL].concat();
            let expected = closed(head, target, QUERY_TAIL);
            let one_call = write_once(&input, target.name);
            assert!(one_call == expected, "{:?}", text(&one_call));
            for split in 0..=input.len() {
                let ctx = format!("{:?} + {} split at {split}", text(head), target.name);
                let (a, b) = input.split_at(split);
                let (ring, filter) = write_split(a, b);
                assert!(
                    ring == one_call,
                    "{ctx}: wrote {:?}, one call writes {:?}",
                    text(&ring),
                    text(&one_call)
                );
                assert!(filter.pending().is_empty(), "{ctx}");
                assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-2 (d): the CSI query constructs split at every position. The write filter
/// never holds a CSI byte, so a split query may be written as its bytes with DEL
/// in place of the final byte instead of being removed whole; the replay matches
/// the raw stream (rows, cursor, no response) wherever the split falls.
#[test]
fn open_body_closure_a_split_csi_query_after_a_body_head_replays_like_the_raw_stream() {
    let start = Instant::now();
    for head in BODY_HEADS {
        for target in TARGETS.iter().filter(|t| !t.held) {
            let input = [*head, target.bytes, QUERY_TAIL].concat();
            for split in 0..=input.len() {
                let ctx = format!("{:?} + {} split at {split}", text(head), target.name);
                let (a, b) = input.split_at(split);
                let (ring, filter) = write_split(a, b);
                assert!(filter.pending().is_empty(), "{ctx}");
                assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
                assert_replays_like_raw(&ring, &[*head, target.bytes].concat(), QUERY_TAIL, &ctx);
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-2 (d), TM-1, through the themed client: every split of an OSC color head,
/// each removed construct and `?` BEL replays with no response, and the rows and
/// cursor equal the raw stream's.
#[cfg(feature = "gui")]
#[test]
fn open_body_closure_no_split_position_lets_a_color_query_join_a_color_head() {
    let start = Instant::now();
    for head in COLOR_HEADS {
        for target in TARGETS {
            let input = [*head, target.bytes, QUERY_TAIL].concat();
            let reference = themed_view_after_a_cut(&[*head, target.bytes].concat(), QUERY_TAIL);
            assert!(
                reference.responses.is_empty(),
                "{:?} + {}: the raw stream answers",
                text(head),
                target.name
            );
            for split in 0..=input.len() {
                let ctx = format!("{:?} + {} split at {split}", text(head), target.name);
                let (a, b) = input.split_at(split);
                let (ring, _filter) = write_split(a, b);
                let replayed = themed_view_of(&ring);
                assert!(
                    replayed.responses.is_empty(),
                    "{ctx}: the replay answers {:?}; ring {:?}",
                    replayed.responses,
                    text(&ring)
                );
                assert_eq!(replayed.rows, reference.rows, "{ctx}: rows");
                assert_eq!(replayed.cursor, reference.cursor, "{ctx}: cursor");
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-3 (FR1, FR7 a/c/d, NFR3, TM-1): the snapshot replay ───────────────

/// AC-3: for each color head and each construct, one cut-free write-filter call of
/// head + construct + `?` BEL writes exactly head + closure + C0 bytes + `?` BEL to
/// the ring, holding nothing and leaving Ground; the stripped scrollback part of
/// the snapshot over that ring equals the ring.
#[test]
fn open_body_closure_the_ring_of_a_color_head_and_a_removed_construct_is_closed_and_kept() {
    let start = Instant::now();
    for head in COLOR_HEADS {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(head), target.name);
            let fed = [*head, target.bytes, QUERY_TAIL].concat();
            let ring = write_once(&fed, &ctx);
            let expected = closed(head, target, QUERY_TAIL);
            assert!(
                ring == expected,
                "{ctx}: the ring holds {:?}, expected {:?}",
                text(&ring),
                text(&expected)
            );
            let (_payload, stripped) = snapshot_scrollback(&ring, &ctx);
            assert!(
                stripped == ring,
                "{ctx}: the snapshot strip changed the ring {:?} into {:?}",
                text(&ring),
                text(&stripped)
            );
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-3, through the themed client: replaying the snapshot payload of that ring
/// produces no response, and its rows and cursor equal `view_after_a_cut` of
/// head + construct then `?` BEL.
#[cfg(feature = "gui")]
#[test]
fn open_body_closure_a_snapshot_replay_of_a_removal_in_a_color_body_answers_nothing() {
    let start = Instant::now();
    for head in COLOR_HEADS {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(head), target.name);
            let ring = write_once(&[*head, target.bytes, QUERY_TAIL].concat(), &ctx);
            let (payload, _stripped) = snapshot_scrollback(&ring, &ctx);
            let replayed = themed_view_of_a_snapshot(&payload);
            let reference = view_after_a_cut(&[*head, target.bytes].concat(), QUERY_TAIL);
            assert!(
                replayed.responses.is_empty(),
                "{ctx}: replaying the snapshot answers {:?}",
                replayed.responses
            );
            assert_eq!(replayed.rows, reference.rows, "{ctx}: rows");
            assert_eq!(replayed.cursor, reference.cursor, "{ctx}: cursor");
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-3, the DCS and APC heads: the ring is the closed form, the snapshot keeps
/// it, and replaying it through the themed client gives the rows, cursor and
/// responses of the raw stream.
#[cfg(feature = "gui")]
#[test]
fn open_body_closure_a_snapshot_replay_of_a_removal_in_a_dcs_or_apc_body_matches_the_raw_stream() {
    let start = Instant::now();
    for head in ST_HEADS {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(head), target.name);
            let ring = write_once(&[*head, target.bytes, QUERY_TAIL].concat(), &ctx);
            assert!(ring == closed(head, target, QUERY_TAIL), "{ctx}");
            let (payload, stripped) = snapshot_scrollback(&ring, &ctx);
            assert!(stripped == ring, "{ctx}: the snapshot strip keeps the ring");
            let replayed = themed_view_of_a_snapshot(&payload);
            let reference = themed_view_after_a_cut(&[*head, target.bytes].concat(), QUERY_TAIL);
            assert_eq!(replayed.responses, reference.responses, "{ctx}: responses");
            assert!(replayed.responses.is_empty(), "{ctx}");
            assert_eq!(replayed.rows, reference.rows, "{ctx}: rows");
            assert_eq!(replayed.cursor, reference.cursor, "{ctx}: cursor");
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-3 controls: the themed client answers `ESC]11;?BEL` (the closure-less join of
/// an OSC 11 head, a removed construct and `?` BEL) and answers each color head
/// followed by `?` BEL, so the empty response buffers above are evidence of the
/// closure and not of a client that cannot answer.
#[cfg(feature = "gui")]
#[test]
fn open_body_closure_control_the_themed_client_answers_the_closure_less_join_and_every_color_head()
{
    let start = std::time::Instant::now();
    assert!(
        !themed_view_of(b"\x1b]11;?\x07").responses.is_empty(),
        "the themed client answers ESC]11;?BEL"
    );
    for head in COLOR_HEADS {
        let probe = [*head, QUERY_TAIL].concat();
        assert!(
            !themed_view_of(&probe).responses.is_empty(),
            "the themed client answers {:?}",
            text(&probe)
        );
    }
    // The same client shows the closed form as text.
    assert!(
        themed_view_of(b"\x1b]11;\x1b\x18?\x07")
            .responses
            .is_empty()
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-4 (FR7 b/c, TM-1): the live continuation ──────────────────────────

/// The responses a live `?` BEL provokes in the themed client after it replayed the
/// resume snapshot of `ring` (main buffer, no segments, an empty screen), with the
/// replay's own responses discarded the way the GUI discards them. The payload is
/// the resume clear prefix followed by the ring, with nothing appended.
#[cfg(feature = "gui")]
#[track_caller]
fn live_responses_after_the_resume_snapshot(
    ring: &[u8],
    expected_ring: &[u8],
    ctx: &str,
) -> Vec<u8> {
    let (payload, segments) = build_resume_snapshot_bytes(ring, &[], b"", false, DIMS);
    assert!(
        payload == [RESUME_CLEAR, expected_ring].concat(),
        "{ctx}: the payload is the clear prefix and the ring, nothing appended: {:?}",
        text(&payload)
    );
    assert!(segments.is_empty(), "{ctx}");
    let mut client = themed_client(80, 24);
    client.reset_and_replay_segments(&payload, &[]);
    let _discarded_replay_responses = client.take_response();
    client.process_pty_data_fully(QUERY_TAIL);
    client.take_response()
}

/// AC-4: the ring is the write-filter output of head + construct (the closed form).
/// Replaying its resume snapshot and then a live `?` BEL produces no response. The
/// same holds when the raw head + construct is given as the ring: the snapshot-time
/// strip writes the closure.
#[cfg(feature = "gui")]
#[test]
fn open_body_closure_a_live_question_mark_after_a_ring_ending_in_a_removal_answers_nothing() {
    let start = Instant::now();
    for head in COLOR_HEADS {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(head), target.name);
            let raw = [*head, target.bytes].concat();
            let ring = write_once(&raw, &ctx);
            let expected_ring = [*head, STRING_BODY_CLOSING, target.c0].concat();
            assert!(ring == expected_ring, "{ctx}: the ring {:?}", text(&ring));

            let responses = live_responses_after_the_resume_snapshot(&ring, &ring, &ctx);
            assert!(
                responses.is_empty(),
                "{ctx}: the live `?` BEL after the write-filter ring answers {responses:?}"
            );
            let responses = live_responses_after_the_resume_snapshot(&raw, &ring, &ctx);
            assert!(
                responses.is_empty(),
                "{ctx}: the live `?` BEL after the raw ring answers {responses:?}"
            );
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-4 controls: with a ring of the head alone the same path answers the live `?`
/// BEL, for each color head.
#[cfg(feature = "gui")]
#[test]
fn open_body_closure_control_the_live_question_mark_is_answered_after_a_bare_color_head() {
    let start = std::time::Instant::now();
    for head in COLOR_HEADS {
        let ctx = text(head);
        let responses = live_responses_after_the_resume_snapshot(head, head, &ctx);
        assert!(
            !responses.is_empty(),
            "{ctx}: the live `?` BEL after a ring of the head alone is answered"
        );
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-6 (FR5, FR6, TM-2): no double closure, and the closure stays inert ─

/// AC-6: after a write-filter call of head + construct writes head + closure + C0
/// bytes, one cut and two cuts at the end of that call, a cut at fed offset 0 of the
/// next call and the reader's fallback closing each write nothing more and leave
/// Ground with no designator awaited.
#[test]
fn open_body_closure_a_cut_after_the_strips_closure_writes_nothing_more() {
    let start = Instant::now();
    for head in BODY_HEADS {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(head), target.name);
            let fed = [*head, target.bytes].concat();
            let expected = [*head, STRING_BODY_CLOSING, target.c0].concat();

            for cuts in [vec![fed.len()], vec![fed.len(), fed.len()]] {
                let mut filter = ScrollbackWriteFilter::new();
                let out = filter.feed_with_cuts(&fed, DIMS, &cuts).bytes;
                assert!(
                    out == expected,
                    "{ctx}: {} cut(s) at the end wrote {:?}",
                    cuts.len(),
                    text(&out)
                );
                assert!(filter.pending().is_empty(), "{ctx}");
                assert!(!filter.awaiting_designator(), "{ctx}");
                assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
            }

            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed(&fed, DIMS).1;
            assert!(out == expected, "{ctx}: cut-free wrote {:?}", text(&out));
            assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
            // A cut at fed 0 of the next call: only its text is written.
            let mut at_zero = ScrollbackWriteFilter::new();
            at_zero.feed(&fed, DIMS);
            let written = at_zero.feed_with_cuts(AFTER, DIMS, &[0]).bytes;
            assert!(
                written == AFTER,
                "{ctx}: a cut at fed 0 wrote {:?}",
                text(&written)
            );
            assert!(at_zero.pending().is_empty(), "{ctx}");
            assert!(!at_zero.awaiting_designator(), "{ctx}");
            assert_eq!(at_zero.written_state(), WrittenState::Ground, "{ctx}");
            // The reader's fallback closing: an empty fed range with a cut at 0.
            let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
            assert!(
                closing.is_empty(),
                "{ctx}: the fallback closing wrote {:?}",
                text(&closing)
            );
            assert!(filter.pending().is_empty(), "{ctx}");
            assert!(!filter.awaiting_designator(), "{ctx}");
            assert_eq!(filter.written_state(), WrittenState::Ground, "{ctx}");
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-6: neither strip reads the closure the strip wrote as a target start or as
/// ST. `body` + closure + each construct becomes `body` + closure + the
/// construct's C0 bytes (no closure is added: the state after the closure is
/// Ground), and `body` + closure alone is kept unchanged.
#[test]
fn open_body_closure_neither_strip_reads_the_written_closure_as_a_target_or_as_st() {
    let start = Instant::now();
    type Strip = fn(&[u8]) -> Vec<u8>;
    let strips: [(&str, Strip); 2] = [
        ("write path", strip_pty_output_for_scrollback_write),
        ("snapshot time", strip_replayable_rich_content),
    ];
    for (strip_name, strip) in strips {
        for head in BODY_HEADS {
            let kept = [*head, STRING_BODY_CLOSING].concat();
            assert_eq!(
                strip(&kept),
                kept,
                "{strip_name}, {:?}: the body and the closure alone are kept",
                text(head)
            );
            for target in TARGETS {
                let input = [&kept[..], target.bytes].concat();
                let expected = [&kept[..], target.c0].concat();
                let out = strip(&input);
                assert!(
                    out == expected,
                    "{strip_name}, {:?} + {}: wrote {:?}, expected {:?}",
                    text(head),
                    target.name,
                    text(&out),
                    text(&expected)
                );
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-6: the vt100 replay copy of a ring holding the closure equals the ring (the
/// closure holds no DEL, which is the only byte the copy rewrites).
#[test]
fn open_body_closure_the_vt100_replay_copy_keeps_the_closure() {
    let start = Instant::now();
    for head in BODY_HEADS {
        for target in TARGETS {
            for cont in CONTINUATIONS {
                let ctx = format!("{:?} + {}", text(head), target.name);
                let ring = write_once(&[*head, target.bytes, *cont].concat(), &ctx);
                assert!(
                    ring == closed(head, target, cont),
                    "{ctx}: the ring {:?}",
                    text(&ring)
                );
                assert!(
                    vt100_replay_copy(&ring) == ring,
                    "{ctx}: the replay copy changed the ring"
                );
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-7 (FR2, FR10): the single definition and the doc comments ─────────

/// The crate's `.rs` sources under `src-tauri/src`: path (relative to that
/// directory) and text.
fn crate_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("reading {dir:?}: {e}"))
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let source = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
                out.push((relative, source));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out
}

/// The source text of one crate file, by its path relative to `src-tauri/src`.
fn crate_source(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"))
}

/// The needle of a constant definition. Built from two pieces so that this file
/// does not count as a definition itself.
fn definition_of(name_tail: &str) -> String {
    format!("const {name_tail}")
}

const SHARED_STRIP: &str = "mux/scrollback_filter.rs";
const WRITE_FILTER: &str = "mux/ipc/pty_spawn/write_filter.rs";

/// AC-7: `STRING_BODY_CLOSING` has exactly one definition in the crate source. It
/// is in `scrollback_filter.rs`, its value is `0x1B 0x18` and it has the same
/// visibility as `CSI_CLOSING`. `write_filter.rs` has no definition, only a
/// same-named re-export, which `closure_for` uses.
#[test]
fn open_body_closure_string_body_closing_is_defined_once_in_the_shared_strip() {
    let start = Instant::now();
    let needle = definition_of(concat!("STRING_BODY", "_CLOSING:"));
    let mut definitions: Vec<(String, String)> = Vec::new();
    for (path, source) in crate_sources() {
        for line in source.lines() {
            if !line.trim_start().starts_with("//") && line.contains(&needle) {
                definitions.push((path.clone(), line.to_string()));
            }
        }
    }
    assert_eq!(
        definitions.len(),
        1,
        "exactly one definition of the closure constant: {definitions:?}"
    );
    let (path, line) = &definitions[0];
    assert_eq!(path, SHARED_STRIP, "the definition is in the shared strip");
    assert!(
        line.contains("&[0x1b, 0x18]"),
        "the value is ESC then CAN: {line}"
    );

    let shared = crate_source(SHARED_STRIP);
    let csi_line = shared
        .lines()
        .find(|line| line.contains(&definition_of("CSI_CLOSING:")))
        .expect("the shared strip defines CSI_CLOSING");
    let visibility = |line: &str| line[..line.find("const ").unwrap()].trim().to_string();
    assert_eq!(
        visibility(line),
        visibility(csi_line),
        "the same visibility as CSI_CLOSING"
    );

    let filter = crate_source(WRITE_FILTER);
    assert!(
        !filter.lines().any(|line| line.contains(&needle)),
        "the write filter has no definition"
    );
    assert!(
        filter.lines().any(|line| {
            line.trim_start().starts_with("pub(in crate::mux) use")
                && line.contains("scrollback_filter::STRING_BODY_CLOSING")
        }),
        "the write filter re-exports the shared constant under the same name"
    );
    assert!(
        filter.contains("WrittenState::OscBody | WrittenState::StBody => STRING_BODY_CLOSING,"),
        "closure_for uses the name"
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-7: the write filter's constant is the shared one, and the closure is ESC
/// followed by CAN.
#[test]
fn open_body_closure_the_write_filter_constant_is_the_shared_one() {
    let start = std::time::Instant::now();
    assert_eq!(
        crate::mux::ipc::pty_spawn::write_filter::STRING_BODY_CLOSING,
        crate::mux::scrollback_filter::STRING_BODY_CLOSING
    );
    assert_eq!(
        crate::mux::scrollback_filter::STRING_BODY_CLOSING,
        &[0x1b, 0x18][..]
    );
    assert_eq!(STRING_BODY_CLOSING, &[0x1b, 0x18][..]);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-7: the shared strip module never depends on the IPC layer: no `use` line
/// names it, and no comment mentions the write filter's `ESCAPE_CLOSING`.
#[test]
fn open_body_closure_the_shared_strip_does_not_depend_on_the_ipc_layer() {
    let start = std::time::Instant::now();
    let shared = crate_source(SHARED_STRIP);
    assert!(
        !shared
            .lines()
            .map(str::trim_start)
            .filter(|line| !line.starts_with("//"))
            .filter(|line| line.starts_with("use ")
                || line.starts_with("pub") && line.contains(" use "))
            .any(|line| line.contains("ipc")),
        "a use line names the IPC layer"
    );
    assert!(
        !comment_text(&shared).contains("ESCAPE_CLOSING"),
        "a comment mentions ESCAPE_CLOSING"
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-7 (FR10): the comments that used to say a removal leaves a written body
/// open, or that nothing is written after a complete string or in a body, state
/// the string-body closure now. The stale phrases are absent, the new vocabulary
/// is present, and the `WrittenState` doc and the write filter's "Post-strip
/// state" paragraph name `STRING_BODY_CLOSING`. Comment lines are joined, so a
/// phrase wrapped over two lines is matched.
#[test]
fn open_body_closure_the_doc_comments_state_the_closure_at_a_removal() {
    let start = std::time::Instant::now();
    let shared_source = crate_source(SHARED_STRIP);
    let shared = comment_text(&shared_source);
    let filter = comment_text(&crate_source(WRITE_FILTER));

    let stale: &[(&str, &str, &str)] = &[
        (
            "scrollback_filter.rs",
            &shared,
            "In ground, or after a complete string, nothing is written",
        ),
        (
            "scrollback_filter.rs",
            &shared,
            "ground for [`WrittenState`]",
        ),
        (
            "scrollback_filter.rs",
            &shared,
            "In ground nothing is added",
        ),
        (
            "scrollback_filter.rs",
            &shared,
            "D1 writes nothing there and the body stays open",
        ),
        (
            "scrollback_filter.rs",
            &shared,
            "the closing byte D1 inserts",
        ),
        (
            "write_filter.rs",
            &filter,
            "such a construct does not end a written string body",
        ),
        (
            "write_filter.rs",
            &filter,
            "by a strip that removes the construct whose `ESC` aborted a written string",
        ),
        (
            "write_filter.rs",
            &filter,
            "The write a cut makes when the written stream ends inside an open OSC body",
        ),
        (
            "write_filter.rs",
            &filter,
            "so can the strip when it removes the construct whose `ESC` aborted a written string",
        ),
        ("write_filter.rs", &filter, "or one a removal left open"),
    ];
    for (file, comments, phrase) in stale {
        assert!(
            !comments.contains(phrase),
            "{file}: a comment still says {phrase:?}"
        );
    }

    let required: &[(&str, &str, &str)] = &[
        ("scrollback_filter.rs", &shared, "OSC body"),
        ("scrollback_filter.rs", &shared, "DCS / APC body"),
        ("scrollback_filter.rs", &shared, "STRING_BODY_CLOSING"),
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

    // The `WrittenState` doc: the comment lines right above the enum.
    let lines: Vec<&str> = shared_source.lines().collect();
    let enum_line = lines
        .iter()
        .position(|line| line.contains("pub(in crate::mux) enum WrittenState"))
        .expect("the shared strip defines WrittenState");
    let doc_start = lines[..enum_line]
        .iter()
        .rposition(|line| {
            !(line.trim_start().starts_with("///") || line.trim_start().starts_with("#["))
        })
        .map_or(0, |index| index + 1);
    let written_state_doc = comment_text(&lines[doc_start..enum_line].join("\n"));
    assert!(
        written_state_doc.contains("STRING_BODY_CLOSING"),
        "the WrittenState doc states the string-body closure at a removal"
    );

    // The write filter's "Post-strip state" paragraph.
    let paragraph_start = filter
        .find("**Post-strip state")
        .expect("the write filter documents the post-strip state");
    let paragraph_len = filter[paragraph_start..]
        .find("**Overflow.**")
        .expect("the Overflow paragraph follows");
    let paragraph = &filter[paragraph_start..paragraph_start + paragraph_len];
    assert!(
        paragraph.contains("STRING_BODY_CLOSING"),
        "the Post-strip state paragraph states the string-body closure at a removal"
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-8 and AC-9 (FR8, FR9): the records ────────────────────────────────

/// The text of a file under the repository root.
fn repository_file(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"))
}

const OLD_NAME: &str =
    "overflow_open_string_cut_a_non_overflow_strip_then_cut_closes_the_open_osc_body";
const NEW_NAME: &str = "overflow_open_string_cut_a_non_overflow_strip_closes_the_osc_body_at_the_removal_and_the_cut_adds_nothing";
const PREDECESSOR_RECORD: &str =
    "test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml";

/// AC-8 item 2: in the predecessor's test-docs record the AC-11 entry lists the
/// successor name, carries a supersede comment naming this feature's SPEC and FR1
/// and FR5, keeps its `red_reason`, and the old name is gone; no other entry names
/// this feature.
#[test]
fn open_body_closure_the_predecessor_record_lists_the_renamed_test_with_a_supersede_note() {
    let start = std::time::Instant::now();
    let record = repository_file(PREDECESSOR_RECORD);
    assert!(!record.contains(OLD_NAME), "the old name is still listed");
    let block_start = record.find("\n  AC-11:\n").expect("the AC-11 entry exists") + 1;
    let block = &record[block_start..];
    let block = block
        .match_indices("\n  AC-")
        .nth(0)
        .map_or(block, |(index, _)| &block[..index]);
    assert!(
        block.contains(&format!(
            "      - mux::ipc::pty_spawn::tests::overflow_open_string_cut::{NEW_NAME}\n"
        )),
        "AC-11 lists the successor name"
    );
    let supersede = block
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(|line| line.trim_start().trim_start_matches('#').trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        supersede.starts_with("Superseded:"),
        "AC-11 carries a supersede comment"
    );
    assert!(
        supersede.contains("feature-docs/mux-strip-open-string-body-closure/SPEC.md FR1/FR5"),
        "the comment names the SPEC and FR1 / FR5: {supersede}"
    );
    assert!(
        block.contains(
            "Observed before the production change: failed at the written-state assertion after the call"
        ) && block.contains("carrying `ESC ]0;x` and `ESC [6n` (the filter reported Ground, the open OSC body expected)."),
        "the red_reason is unchanged"
    );
    assert_eq!(
        record.matches("mux-strip-open-string-body-closure").count(),
        1,
        "no other entry names this feature"
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-9 (FR9): DECISIONS.md records the closure-byte decision, the before and
/// after value of every expectation changed under AC-8, the rename with the
/// test-docs record update, and that Residual 1 of mux-strip-join-escape-closure is
/// resolved by this feature.
#[test]
fn open_body_closure_the_decision_record_states_the_four_things() {
    let start = std::time::Instant::now();
    let record = repository_file("feature-docs/mux-strip-open-string-body-closure/DECISIONS.md");
    let required: &[(&str, &str)] = &[
        // The closure-byte decision.
        ("the closure byte", "STRING_BODY_CLOSING"),
        ("the closure byte", "ESC then CAN"),
        ("the closure byte", "before the re-emitted C0 bytes"),
        // The changed expectations, by test name, with their before and after.
        ("the rename", OLD_NAME),
        ("the rename", NEW_NAME),
        (
            "item 3",
            "strip_concat_a_construct_removed_in_ground_adds_no_closing",
        ),
        (
            "item 4",
            "strip_concat_a_construct_removed_in_ground_or_a_kept_string_adds_no_closing",
        ),
        (
            "item 5",
            "post_strip_state_form_reports_the_csi_state_of_the_written_bytes",
        ),
        (
            "item 6",
            "post_strip_state_form_output_equals_the_write_path_strip",
        ),
        ("before", "Before"),
        ("after", "After"),
        // The test-docs record update.
        ("the record update", PREDECESSOR_RECORD),
        ("the record update", "AC-11"),
        // Residual 1.
        ("Residual 1", "mux-strip-join-escape-closure"),
        ("Residual 1", "Residual 1"),
        ("Residual 1", "resolved"),
    ];
    for (what, phrase) in required {
        assert!(
            record.contains(phrase),
            "DECISIONS.md lacks {phrase:?} ({what})"
        );
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}
