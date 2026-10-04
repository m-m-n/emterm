//! mux-snapshot-strip-can-abort task0001 (AC-2, AC-9): the write filter to
//! snapshot round trip of an open Kitty APC / SIXEL DCS body closed at a cut, and
//! the comment text of the files the feature touches.
//!
//! The overflow flush writes a Kitty APC / SIXEL DCS body the strip never closed,
//! and a later cut writes `ESC` + CAN after it. The ring then holds the body, the
//! closure and the bytes written after the cut. A snapshot strip of that ring
//! ends the body at the closure's `ESC` as an abort: it removes the body up to
//! that `ESC`, keeps the closure and judges every byte after it by the ordinary
//! rules, so the text written after the cut reaches the replay. The strip used to
//! read the body as spanning to the next ST and removed that text with it.
//!
//! Oracle convention (the one overflow_open_string_cut uses): replay checks
//! compare term_core views (rows, cursor, responses); the reference is term_core
//! fed the raw stream with a 47 / 1047 / 1049 `h` / `l` pair in place of the cut.
//! A held body is about 0.5 MiB: each test builds it once and keeps the number of
//! passes over it small. The payloads neither answer nor place an image.

use super::overflow_open_string_cut::{
    AFTER, Body, comment_text, overflow, shows_after_text, st_bodies,
};
use super::post_strip_cut_csi::BUDGET;
use super::round3_write_path::{DIMS, switch_pairs};
use super::round4_cut_csi::{text, view_after_a_cut, view_of};
use super::*;
use crate::mux::scrollback_filter::strip_replayable_rich_content;

/// The ST-terminated OSC 0 written after the cut.
const OSC0: &[u8] = b"\x1b]0;title\x1b\\";

/// AC-2 (FR1, FR2, FR3, FR6; TM-1) for one body: held at the cap and flushed open
/// past it, then a call carrying a cut at fed 0 followed by plain text and an
/// ST-terminated OSC 0. The ring is the bytes the filter wrote, in order. The
/// snapshot strip of the ring gives `ESC` + CAN, the plain text and the OSC 0
/// bytes; term_core replaying that output shows the plain text, and its view
/// equals the view of the raw stream with each 47 / 1047 / 1049 pair in place of
/// the cut.
fn check_the_ring_of_a_body_closed_at_a_cut_strips_to_what_follows_the_cut(body: &Body) {
    let start = std::time::Instant::now();
    let name = body.name;
    let held = body.held_at_the_cap();
    let more = body.ten_more();

    let (mut filter, flush) = overflow(&held, &more);
    assert_eq!(
        filter.written_state(),
        WrittenState::StBody,
        "{name}: the flush leaves the body open"
    );
    let later = [AFTER, OSC0].concat();
    let second = filter.feed_with_cuts(&later, DIMS, &[0]);
    assert!(
        second.bytes == [STRING_BODY_CLOSING, &later[..]].concat(),
        "{name}: the cut writes ESC and CAN, then the text and the OSC"
    );
    assert_eq!(filter.written_state(), WrittenState::Ground, "{name}");

    let ring = [&flush.bytes[..], &second.bytes[..]].concat();
    let stripped = strip_replayable_rich_content(&ring);
    assert!(
        stripped == [STRING_BODY_CLOSING, AFTER, OSC0].concat(),
        "{name}: the snapshot strip gives ESC and CAN, the text and the OSC 0 \
         (got {} bytes)",
        stripped.len()
    );

    let replay = view_of(&stripped);
    assert!(
        shows_after_text(&replay),
        "{name}: term_core replaying the stripped ring shows the text"
    );
    assert!(replay.responses.is_empty(), "{name}: no response");
    let raw_before = [&held[..], &more[..]].concat();
    for (enter, leave) in switch_pairs() {
        let reference = view_after_a_cut(&[&raw_before[..], enter, leave].concat(), &later);
        assert_eq!(
            replay,
            reference,
            "{name}, switch {}: the replay of the stripped ring matches the raw stream \
             with the switch for the cut",
            text(enter)
        );
    }
    assert!(
        start.elapsed() < BUDGET,
        "{name}: took {:?}",
        start.elapsed()
    );
}

#[test]
fn snapshot_strip_can_abort_a_kitty_apc_closed_at_a_cut_leaves_the_text_after_it() {
    check_the_ring_of_a_body_closed_at_a_cut_strips_to_what_follows_the_cut(&st_bodies()[2]);
}

#[test]
fn snapshot_strip_can_abort_a_sixel_dcs_closed_at_a_cut_leaves_the_text_after_it() {
    check_the_ring_of_a_body_closed_at_a_cut_strips_to_what_follows_the_cut(&st_bodies()[3]);
}

/// AC-9 (FR7): the comments that stated the old open-body behavior describe the
/// abort-aware body end. Read with the same comment extraction as
/// `overflow_open_string_cut_the_doc_comments_state_the_open_body_model`.
#[test]
fn snapshot_strip_can_abort_the_comments_describe_the_abort_aware_body_end() {
    let strip = comment_text(include_str!("../../../scrollback_filter.rs"));
    let filter = comment_text(include_str!("../write_filter.rs"));
    let cut_tests = comment_text(include_str!("overflow_open_string_cut.rs"));

    let stale: &[(&str, &str, &str)] = &[
        (
            "write_filter.rs",
            &filter,
            "or ends an APC / DCS body at it",
        ),
        ("scrollback_filter.rs", &strip, "no more ST terminators"),
        (
            "overflow_open_string_cut.rs",
            &cut_tests,
            "spans to the next ST",
        ),
    ];
    for (file, comments, phrase) in stale {
        assert!(
            !comments.contains(phrase),
            "{file}: a comment still says {phrase:?}"
        );
    }

    let required: &[(&str, &str, &str)] = &[("scrollback_filter.rs", &strip, "aborting `ESC`")];
    for (file, comments, phrase) in required {
        assert!(
            comments.contains(phrase),
            "{file}: no comment says {phrase:?}"
        );
    }
}
