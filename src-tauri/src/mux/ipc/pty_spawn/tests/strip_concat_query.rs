//! mux-strip-concat-query-closure task0001 (FR1-FR7, NFR1-NFR3): the join the
//! shared strip produces never completes an escape or a CSI device query the
//! raw stream never made.
//!
//! The write filter and the production reader are driven here; the strip-level
//! cases of the same feature live in `scrollback_filter::tests`.
//!
//! Oracle convention (SPEC A3): the reference is term_core fed the raw stream,
//! with the removed construct's own effect kept out. A construct's own effect is
//! the answer an answered CSI query provokes in the raw stream; the ring never
//! replays it. The comparisons therefore replay the written ring and compare it
//! with the raw stream for what the bytes AFTER the construct provoke
//! (`view_after_a_cut`), and the reader-level cases use the visibility-restore
//! harness of `round3_write_path` with the own-answer helper of
//! `post_strip_cut_csi`.
//!
//! Test identifiers (task plan): TS-2 the one-call closing, TS-3 the reader
//! level, TS-4 the split device queries, TS-5 the classification parity, TS-6
//! the lone ESC, TS-9 the budgets.

use super::post_strip_cut_csi::{assert_client_equals_reference_except, client_csi_phase};
use super::round3_write_path::{
    DIMS, new_core, reference_view, run_reader_without_owner, run_visibility_restore_at,
};
use super::round4_cut_csi::{osc_held_at_the_cap, text, view_after_a_cut, view_of};
use super::*;
use crate::mux::scrollback_filter::{
    strip_pty_output_for_scrollback_write, strip_replayable_rich_content,
};
use crate::mux::snapshot_bytes::build_snapshot_bytes;
use std::time::{Duration, Instant};
use term_core::terminal_core::TerminalCore;

const BUDGET: Duration = Duration::from_secs(10);

const DEL: u8 = 0x7f;
const ESC: u8 = 0x1b;

/// A construct the shared strip removes together with its opening `ESC`.
pub(super) struct Target {
    pub(super) name: &'static str,
    pub(super) bytes: &'static [u8],
    /// The C0 bytes the strip re-emits from the removed construct, in order.
    pub(super) c0: &'static [u8],
    /// The write filter holds it across calls until it is complete. A CSI
    /// query is never held: its bytes are written as they arrive.
    pub(super) held: bool,
}

impl Target {
    fn is_query(&self) -> bool {
        !self.held
    }
}

pub(super) const TARGETS: &[Target] = &[
    Target {
        name: "osc 777 launch (BEL)",
        bytes: b"\x1b]777;emterm;markdown;begin;id=x\x07",
        c0: b"",
        held: true,
    },
    Target {
        name: "osc 777 launch (ST)",
        bytes: b"\x1b]777;emterm;markdown;begin;id=x\x1b\\",
        c0: b"",
        held: true,
    },
    Target {
        name: "osc 9999 emterm-md",
        bytes: b"\x1b]9999;emterm-md;begin\x1b\\",
        c0: b"",
        held: true,
    },
    Target {
        name: "agent-status",
        bytes: b"\x1b]777;emterm;agent-status;v=1;state=idle\x07",
        c0: b"",
        held: true,
    },
    Target {
        // The payload neither answers nor places an image (EC-5 of the
        // predecessor feature).
        name: "kitty apc",
        bytes: b"\x1b_Gi=1,a=d;AAAA\x1b\\",
        c0: b"",
        held: true,
    },
    Target {
        name: "sixel dcs",
        bytes: b"\x1bPq#0;2;0;0;0\x1b\\",
        c0: b"",
        held: true,
    },
    Target {
        name: "csi query",
        bytes: b"\x1b[6n",
        c0: b"",
        held: false,
    },
    Target {
        name: "csi query with an embedded CR",
        bytes: b"\x1b[6\rn",
        c0: b"\r",
        held: false,
    },
];

/// The open CSIs of the plan: the entry state, parameters, a private marker, an
/// intermediate, and text before.
const HEADS: &[&[u8]] = &[
    b"\x1b[",
    b"\x1b[6",
    b"\x1b[?25",
    b"\x1b[6 ",
    b"abc\x1b[12;3",
];

/// What follows the removed construct: finals that would complete a query with
/// the open head, a non-query final, and text.
const CONTINUATIONS: &[&[u8]] = &[b"n", b"c", b"t", b"m", b"text"];

/// The answered forms of AC-3.
const ANSWERED_FORMS: &[&[u8]] = &[
    b"\x1b[6n",
    b"\x1b[5n",
    b"\x1b[261n",
    b"\x1b[c",
    b"\x1b[0c",
    b"\x1b[?c",
    b"\x1b[>c",
    b"\x1b[14t",
    b"\x1b[16t",
    b"\x1b[18t",
    b"\x1b[?1$p",
];

/// CSIs term_core does not answer (EC-9): written unchanged however they are
/// split.
const NON_QUERY_FORMS: &[&[u8]] = &[
    b"\x1b[31m",
    b"\x1b[=c",
    b"\x1b[0n",
    b"\x1b[15t",
    b"\x1b[?25h",
    b"\x1b[1;2H",
    b"\x1b[8;24;80t",
    b"\x1b[ q",
    b"\x1b[>1p",
    b"\x1b[6;2m",
];

/// C0 bytes that execute inside a CSI: CR, LF, BS, BEL.
const C0_BYTES: &[u8] = &[0x0d, 0x0a, 0x08, 0x07];

// ── helpers ──────────────────────────────────────────────────────────────

/// What the ring holds for an answered `form` once its final byte is written in
/// a later call than the rest: the same bytes with DEL in place of the final.
fn closed_form(form: &[u8]) -> Vec<u8> {
    [&form[..form.len() - 1], CSI_CLOSING].concat()
}

/// Replaying `ring` gives no response and the rows and cursor of the raw
/// stream `raw_before` followed by `after`, with the answers to `raw_before`
/// (the removed construct's own effect) discarded.
#[track_caller]
fn assert_replays_like_raw(ring: &[u8], raw_before: &[u8], after: &[u8], ctx: &str) {
    let replayed = view_of(ring);
    let reference = view_after_a_cut(raw_before, after);
    assert!(
        replayed.responses.is_empty(),
        "{ctx}: replaying the ring answers {:?}; ring {:?}",
        replayed.responses,
        text(ring)
    );
    assert!(
        reference.responses.is_empty(),
        "{ctx}: the raw stream answers {:?} after the construct",
        reference.responses
    );
    assert_eq!(
        replayed.rows,
        reference.rows,
        "{ctx}: rows differ; ring {:?}",
        text(ring)
    );
    assert_eq!(
        replayed.cursor,
        reference.cursor,
        "{ctx}: cursor differs; ring {:?}",
        text(ring)
    );
}

/// A lone trailing `ESC` is the only thing the filter may hold after a call
/// that ends in a CSI byte or in ground: CSI bytes are never held (NFR1).
#[track_caller]
fn assert_only_a_lone_esc_is_held(filter: &ScrollbackWriteFilter, ctx: &str) {
    assert!(
        filter.pending().is_empty() || filter.pending() == [ESC],
        "{ctx}: pending holds {:?}",
        text(filter.pending())
    );
}

/// Feed `pieces` as consecutive cut-free calls; after every call the carried
/// CSI state is term_core's on the bytes written so far (it is never an escape
/// state), and nothing but a lone `ESC` is held. Returns the bytes written.
#[track_caller]
fn feed_checked(pieces: &[&[u8]], ctx: &str) -> Vec<u8> {
    let mut filter = ScrollbackWriteFilter::new();
    let mut emitted = Vec::new();
    for (idx, piece) in pieces.iter().enumerate() {
        emitted.extend_from_slice(&filter.feed(piece, DIMS).1);
        assert!(!filter.awaiting_designator(), "{ctx}: call {idx}");
        assert_eq!(
            filter.csi_phase(),
            client_csi_phase(&emitted),
            "{ctx}: after call {idx} the carried state is term_core's on {:?}",
            text(&emitted)
        );
        if idx + 1 == pieces.len() {
            // The held ESC chain may stand at the very end only when the fed
            // stream ends in an ESC.
            assert!(
                filter.pending().is_empty() || piece.ends_with(&[ESC]),
                "{ctx}: pending holds {:?} at the end",
                text(filter.pending())
            );
        }
    }
    emitted
}

fn own_answer_of(prefix: &[u8]) -> Vec<u8> {
    let mut core = new_core();
    core.process_pty_data_fully(prefix);
    core.take_response()
}

/// Whether term_core answers `stream`.
fn answers(stream: &[u8]) -> bool {
    let mut core = TerminalCore::new(80, 24, 10);
    core.process_pty_data_fully(stream);
    !core.take_response().is_empty()
}

// ── TS-2 (AC-1, FR1, FR5): one cut-free call ─────────────────────────────

/// AC-1 (FR1, FR5, NFR3, TM-1, TS-2): an open head, a removed construct of every
/// kind and a continuation, in one cut-free write-filter call. Exactly one
/// CSI_CLOSING sits at the construct's position, before any re-emitted C0 byte
/// (EC-3); `ESC[` + launch + `c` forms no DA1 (EC-5); no CSI is open
/// afterwards; replaying the ring through term_core gives no response and the
/// rows and cursor of the raw stream with the construct's own effect excluded.
#[test]
fn strip_concat_one_call_closes_an_open_csi_at_every_removed_construct() {
    for head in HEADS {
        for target in TARGETS {
            for cont in CONTINUATIONS {
                let ctx = format!("{:?} + {} + {:?}", text(head), target.name, text(cont));
                let fed = [*head, target.bytes, *cont].concat();
                let expected = [*head, CSI_CLOSING, target.c0, *cont].concat();
                let mut filter = ScrollbackWriteFilter::new();
                let out = filter.feed(&fed, DIMS).1;
                assert!(
                    out == expected,
                    "{ctx}: written {:?}, expected {:?}",
                    text(&out),
                    text(&expected)
                );
                assert_eq!(
                    out.iter().filter(|b| **b == DEL).count(),
                    1,
                    "{ctx}: exactly one closing"
                );
                assert!(filter.pending().is_empty(), "{ctx}");
                assert!(!filter.awaiting_designator(), "{ctx}");
                assert_eq!(filter.csi_phase(), None, "{ctx}: no CSI is open");
                assert_eq!(client_csi_phase(&out), None, "{ctx}: term_core agrees");
                assert_replays_like_raw(&out, &[*head, target.bytes].concat(), cont, &ctx);
            }
        }
    }
}

/// EC-4 (FR1, TS-2): several removed constructs inside one open CSI produce
/// exactly one closing, and the C0 bytes a removed query re-emits follow it, in
/// order, once.
#[test]
fn strip_concat_several_constructs_in_one_open_csi_write_one_closing() {
    let all: Vec<u8> = TARGETS.iter().flat_map(|t| t.bytes.to_vec()).collect();
    let c0: Vec<u8> = TARGETS.iter().flat_map(|t| t.c0.to_vec()).collect();
    for head in HEADS {
        let fed = [*head, &all[..], b"n"].concat();
        let expected = [*head, CSI_CLOSING, &c0[..], b"n"].concat();
        let mut filter = ScrollbackWriteFilter::new();
        let out = filter.feed(&fed, DIMS).1;
        assert!(out == expected, "{:?}: {:?}", text(head), text(&out));
        assert_eq!(filter.csi_phase(), None);
        assert_replays_like_raw(&out, &[*head, &all[..]].concat(), b"n", &text(head));
    }

    // Two queries that each embed a C0 byte: the closing precedes both, and
    // the second adds none.
    let fed = b"\x1b[6\x1b[5\rn\x1b[6\x08n!".to_vec();
    let mut filter = ScrollbackWriteFilter::new();
    let out = filter.feed(&fed, DIMS).1;
    assert!(
        out == [&b"\x1b[6"[..], CSI_CLOSING, b"\r\x08!"].concat(),
        "{:?}",
        text(&out)
    );
}

/// FR1 (TS-2): a construct removed in ground, and one removed after a
/// completed CSI, adds no byte; neither does one removed after a kept string
/// body.
#[test]
fn strip_concat_a_construct_removed_in_ground_or_a_kept_string_adds_no_closing() {
    for ground in [
        &b"abc"[..],
        b"\x1b[1m",
        b"abc\x1b[12;3H",
        b"\x1b]0;t\x07",
        b"\x1b]0;t\x1b\\",
        b"\x1b(B",
    ] {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(ground), target.name);
            let fed = [ground, target.bytes, b"n"].concat();
            let expected = [ground, target.c0, b"n"].concat();
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed(&fed, DIMS).1;
            assert!(out == expected, "{ctx}: {:?}", text(&out));
            assert!(!out.contains(&DEL), "{ctx}: no closing");
            assert_replays_like_raw(&out, &[ground, target.bytes].concat(), b"n", &ctx);
        }
    }

    // After a kept string whose body the removed construct's ESC aborts: the
    // written state is ground, so nothing is written (D1).
    for body_head in [&b"\x1b]0;t"[..], b"\x1b_Xnot-kitty"] {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(body_head), target.name);
            let fed = [body_head, target.bytes, b"n"].concat();
            let expected = [body_head, target.c0, b"n"].concat();
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed(&fed, DIMS).1;
            assert!(out == expected, "{ctx}: {:?}", text(&out));
        }
    }
}

/// AC-6 (FR7): a cut that follows a strip-written closing writes no second
/// closing, in a call that ends in the cut, in a cut at fed 0 and in the
/// reader's fallback closing.
#[test]
fn strip_concat_a_cut_after_a_strip_written_closing_writes_no_second_closing() {
    for head in HEADS {
        for target in TARGETS {
            let ctx = format!("{:?} + {}", text(head), target.name);
            let fed = [*head, target.bytes].concat();
            let expected = [*head, CSI_CLOSING, target.c0].concat();

            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes;
            assert!(out == expected, "{ctx}: cut at the end: {:?}", text(&out));
            assert_eq!(filter.csi_phase(), None, "{ctx}");

            let mut filter = ScrollbackWriteFilter::new();
            let out = filter
                .feed_with_cuts(&fed, DIMS, &[fed.len(), fed.len()])
                .bytes;
            assert!(out == expected, "{ctx}: two cuts: {:?}", text(&out));

            let mut filter = ScrollbackWriteFilter::new();
            let mut got = filter.feed(&fed, DIMS).1;
            assert!(got == expected, "{ctx}: cut-free: {:?}", text(&got));
            assert_eq!(
                filter.csi_phase(),
                None,
                "{ctx}: the strip's closing left no CSI"
            );
            got = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
            assert!(got.is_empty(), "{ctx}: the fallback closing finds nothing");
            let got = filter.feed_with_cuts(b"n", DIMS, &[0]).bytes;
            assert!(got == b"n", "{ctx}: a cut at fed 0 finds nothing either");
        }
    }
}

// ── TS-3 (AC-2, FR2): the production reader ──────────────────────────────

/// The client's state equals the raw-stream reference's after a reattach
/// snapshot of the ring after `snapshot_after + 1` reads, followed by the
/// remaining reads live. The responses compared exclude the answer to the
/// removed construct itself (`own`), which the reference gives and the ring
/// never replays.
#[track_caller]
fn assert_reattach_matches_reference(
    chunks: &[Vec<u8>],
    snapshot_after: usize,
    own: &[u8],
    ctx: &str,
) {
    let ring = run_reader_without_owner(chunks[..=snapshot_after].to_vec());
    let (payload, _segments) = build_snapshot_bytes(&ring, &[], b"", false, DIMS);
    let mut client = new_core();
    client.process_pty_data_fully(&payload);
    let replayed = client.take_response();
    let mut responses = Vec::new();
    for chunk in &chunks[snapshot_after + 1..] {
        client.process_pty_data_fully(chunk);
        responses.extend(client.take_response());
    }
    let (reference, reference_responses) = reference_view(chunks);
    let provoked_later = reference_responses
        .strip_prefix(own)
        .unwrap_or_else(|| panic!("{ctx}: the reference's responses lack {own:?}"));
    assert_eq!(
        responses,
        provoked_later,
        "{ctx}: responses after the reattach snapshot; replaying it answered {replayed:?}; \
         ring {:?}",
        text(&ring)
    );
    for r in 0..R2_ROWS {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "{ctx}: row {r}; ring {:?}",
            text(&ring)
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

/// AC-2 (FR2, TM-1, TS-3): `ESC[6` and a removed construct of every kind in one
/// read, `n` (and CR, then `n`) in later reads, the production visibility
/// restore taken after the first read and no screen switch. The client's
/// responses, rows and cursor equal the raw-stream reference's, for the CSI
/// query kinds apart from the reference's own answer to the removed query
/// (the client gives it only when the restore covers the read holding the query
/// and the reader re-delivers it live). The same sequences go through the
/// reattach layout.
#[test]
fn strip_concat_reader_restore_matches_the_raw_stream_reference() {
    let head = &b"\x1b[6"[..];
    for target in TARGETS {
        let first = [head, target.bytes].concat();
        let own = own_answer_of(&first);
        assert_eq!(
            own.is_empty(),
            !target.is_query(),
            "{}: only the query kinds have an answer of their own",
            target.name
        );

        // The restore covers the read with the construct; `n` follows.
        let chunks = vec![first.clone(), b"n".to_vec()];
        let run = run_visibility_restore_at(&chunks, 0);
        assert_client_equals_reference_except(
            &run.received,
            &chunks,
            &own,
            &own,
            &format!("(a) {}", target.name),
        );

        // The same with a CR read between the construct and the `n`.
        let chunks = vec![first.clone(), b"\r".to_vec(), b"n".to_vec()];
        let run = run_visibility_restore_at(&chunks, 0);
        assert_client_equals_reference_except(
            &run.received,
            &chunks,
            &own,
            &own,
            &format!("(b0) {}", target.name),
        );

        // The restore between the construct's read and the CR.
        let run = run_visibility_restore_at(&chunks, 1);
        assert_client_equals_reference_except(
            &run.received,
            &chunks,
            &own,
            &[],
            &format!("(b1) {}", target.name),
        );

        // The reattach layout: a snapshot of the ring after the first read,
        // then the later reads live.
        let chunks = vec![first.clone(), b"n".to_vec()];
        assert_reattach_matches_reference(
            &chunks,
            0,
            &own,
            &format!("reattach (a) {}", target.name),
        );
        let chunks = vec![first.clone(), b"\r".to_vec(), b"n".to_vec()];
        assert_reattach_matches_reference(
            &chunks,
            0,
            &own,
            &format!("reattach (b0) {}", target.name),
        );
        assert_reattach_matches_reference(
            &chunks,
            1,
            &own,
            &format!("reattach (b1) {}", target.name),
        );
    }
}

/// EC-2 (FR2, TM-1, TS-3): the construct is split across reads with the restore
/// between them: the ring ends in `ESC[6` and the filter holds the head of the
/// construct. The client's responses, rows and cursor equal the raw-stream
/// reference's. This depends on the re-delivery of the held head by
/// `suppressed_output`.
#[test]
fn strip_concat_a_held_construct_split_across_reads_matches_the_raw_stream_reference() {
    let head = &b"\x1b[6"[..];
    for target in TARGETS.iter().filter(|t| t.held) {
        let len = target.bytes.len();
        let mut splits = vec![1, 2, len / 2, len - 1];
        splits.sort_unstable();
        splits.dedup();
        for split in splits {
            let (a, b) = target.bytes.split_at(split);
            let chunks = vec![[head, a].concat(), b.to_vec(), b"n".to_vec()];
            for restore in [0, 1] {
                let run = run_visibility_restore_at(&chunks, restore);
                assert_client_equals_reference_except(
                    &run.received,
                    &chunks,
                    &[],
                    &[],
                    &format!(
                        "{} split at {split}, restore at read {restore}",
                        target.name
                    ),
                );
            }
        }
    }
}

// ── TS-4 (AC-3, FR3): a device query split across calls ──────────────────

/// AC-3 (FR3, TM-2, TS-4): each answered form split at every position. The ring
/// never holds an executable query: when the form's final byte arrives in a
/// later call than the rest, it is written as CSI_CLOSING in place of it, and a
/// form that is complete in one call, or whose ESC is held until it joins the
/// rest, is removed whole. Replaying the ring gives no response, the carried
/// state is clear afterwards, and no CSI byte is ever held.
#[test]
fn strip_concat_an_answered_form_split_at_every_position_never_reaches_the_ring_answerable() {
    for form in ANSWERED_FORMS {
        for split in 0..=form.len() {
            let ctx = format!("{:?} split at {split}", text(form));
            let (a, b) = form.split_at(split);
            let mut filter = ScrollbackWriteFilter::new();
            let mut ring = Vec::new();
            for piece in [a, b] {
                ring.extend_from_slice(&filter.feed(piece, DIMS).1);
                assert_only_a_lone_esc_is_held(&filter, &ctx);
            }
            let expected = if split <= 1 || split == form.len() {
                Vec::new()
            } else {
                closed_form(form)
            };
            assert!(
                ring == expected,
                "{ctx}: ring {:?}, expected {:?}",
                text(&ring),
                text(&expected)
            );
            assert!(filter.pending().is_empty(), "{ctx}");
            assert_eq!(filter.csi_phase(), None, "{ctx}: nothing is carried");
            assert!(!answers(&ring), "{ctx}: replaying the ring answers");
            // A later `n` is plain text on replay, as in the raw stream.
            assert!(
                !answers(&[&ring[..], b"n"].concat()),
                "{ctx}: the ring plus `n` answers"
            );
        }
    }
}

/// AC-3 (FR3, TM-2, TS-4): the same forms fed byte by byte: the ring holds the
/// form with DEL in place of the final byte, and only a lone `ESC` is ever
/// held.
#[test]
fn strip_concat_an_answered_form_fed_byte_by_byte_is_closed_in_place_of_its_final_byte() {
    for form in ANSWERED_FORMS {
        let fed = [&b"ab"[..], *form].concat();
        let ctx = format!("{:?}", text(form));
        let mut filter = ScrollbackWriteFilter::new();
        let mut ring = Vec::new();
        for (idx, byte) in fed.iter().enumerate() {
            ring.extend_from_slice(&filter.feed(&[*byte], DIMS).1);
            assert_only_a_lone_esc_is_held(&filter, &format!("{ctx}, byte {idx}"));
        }
        let expected = [&b"ab"[..], &closed_form(form)[..]].concat();
        assert!(
            ring == expected,
            "{ctx}: ring {:?}, expected {:?}",
            text(&ring),
            text(&expected)
        );
        assert_eq!(filter.csi_phase(), None, "{ctx}");
        assert!(!answers(&ring), "{ctx}: replaying the ring answers");
    }
}

/// AC-3 (FR3, TM-2, TS-4, EC-9): C0 bytes inside the continuation of a split
/// answered form keep their effect once and in order, whether they arrive with
/// the rest of the form or in a call of their own, and the final byte is still
/// written as CSI_CLOSING. The ring replays to the screen and cursor of the raw
/// stream, and gives no response.
#[test]
fn strip_concat_c0_bytes_inside_a_split_answered_form_keep_their_effect_once_and_in_order() {
    for form in ANSWERED_FORMS {
        for &c0 in C0_BYTES {
            for split in 2..form.len() {
                let (a, b) = form.split_at(split);
                let ctx = format!("{:?} split at {split}, C0 {c0:#04x}", text(form));
                let first = [&b"ab"[..], a].concat();
                let expected = [&b"ab"[..], a, &[c0][..], &b[..b.len() - 1], CSI_CLOSING].concat();
                let raw = [&b"ab"[..], a, &[c0][..], b].concat();
                let reference = view_of(&raw);

                // The C0 byte arrives with the rest of the form.
                let with_rest = [&[c0][..], b].concat();
                let ring = feed_checked(&[&first[..], &with_rest[..]], &ctx);
                assert!(
                    ring == expected,
                    "{ctx}: with the rest: ring {:?}",
                    text(&ring)
                );
                let replayed = view_of(&ring);
                assert!(replayed.responses.is_empty(), "{ctx}");
                assert_eq!(replayed.rows, reference.rows, "{ctx}: rows");
                assert_eq!(replayed.cursor, reference.cursor, "{ctx}: cursor");

                // The C0 byte in a call of its own.
                let ring = feed_checked(&[&first[..], &[c0][..], b], &ctx);
                assert!(ring == expected, "{ctx}: own call: ring {:?}", text(&ring));

                // The whole stream byte by byte.
                let bytes: Vec<&[u8]> = raw.chunks(1).collect();
                let ring = feed_checked(&bytes, &ctx);
                assert!(
                    ring == expected,
                    "{ctx}: byte by byte: ring {:?}",
                    text(&ring)
                );
            }
        }
    }
}

/// Open heads and the query a later call completes them with (EC-8).
const CARRIED_QUERIES: &[(&[u8], &[u8])] = &[
    (b"\x1b[6", b"\x1b[6n"),
    (b"\x1b[", b"\x1b[c"),
    (b"\x1b[?25", b"\x1b[>c"),
    (b"\x1b[6 ", b"\x1b[5n"),
    (b"abc\x1b[12;3", b"\x1b[?1$p"),
];

/// EC-8 (FR3, FR1, TS-4): `ESC[6` in one call, `ESC[6n` in the next: the second
/// call's strip removes the query from the carried CSI and writes the closing.
#[test]
fn strip_concat_a_query_in_a_later_call_closes_the_csi_carried_in() {
    for (head, query) in CARRIED_QUERIES.iter().copied() {
        let ctx = format!("{:?} then {:?}", text(head), text(query));
        let mut filter = ScrollbackWriteFilter::new();
        let mut ring = filter.feed(head, DIMS).1;
        assert!(filter.csi_phase().is_some(), "{ctx}: the head is open");
        ring.extend_from_slice(&filter.feed(&[query, b"x"].concat(), DIMS).1);
        assert!(
            ring == [head, CSI_CLOSING, b"x"].concat(),
            "{ctx}: {:?}",
            text(&ring)
        );
        assert_eq!(filter.csi_phase(), None, "{ctx}");
        assert_replays_like_raw(&ring, &[head, query].concat(), b"x", &ctx);
    }
}

/// EC-9 (FR3, TS-4): a CSI that completes as a non-query is written unchanged
/// however it is split, and a split query with C0 bytes in between keeps the
/// C0 effect and is closed in place of its final byte.
#[test]
fn strip_concat_a_split_non_query_csi_is_written_unchanged() {
    for form in NON_QUERY_FORMS {
        assert!(!answers(form), "{:?} is not answered", text(form));
        for split in 0..=form.len() {
            let ctx = format!("{:?} split at {split}", text(form));
            let (a, b) = form.split_at(split);
            let ring = feed_checked(&[a, b], &ctx);
            assert!(ring == *form, "{ctx}: {:?}", text(&ring));
        }
        let ctx = format!("{:?} byte by byte", text(form));
        let bytes: Vec<&[u8]> = form.chunks(1).collect();
        let ring = feed_checked(&bytes, &ctx);
        assert!(ring == *form, "{ctx}: {:?}", text(&ring));
    }
}

/// FR3 (TS-4): a cut clears the carried classification. After a cut the next
/// bytes start from ground, so an `n` is written as `n`, not as CSI_CLOSING.
#[test]
fn strip_concat_a_cut_clears_the_carried_classification() {
    // The reader's fallback closing (an empty range with a cut at 0).
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = filter.feed(b"\x1b[6", DIMS).1;
    ring.extend_from_slice(&filter.feed_with_cuts(b"", DIMS, &[0]).bytes);
    assert_eq!(filter.csi_phase(), None);
    ring.extend_from_slice(&filter.feed(b"n", DIMS).1);
    assert!(ring == [&b"\x1b[6"[..], CSI_CLOSING, b"n"].concat());

    // A cut inside the call: the bytes before it run from the carried CSI, the
    // bytes after it from ground.
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = filter.feed(b"\x1b[6", DIMS).1;
    ring.extend_from_slice(&filter.feed_with_cuts(b"\rn", DIMS, &[1]).bytes);
    assert!(
        ring == [&b"\x1b[6\r"[..], CSI_CLOSING, b"n"].concat(),
        "{:?}",
        text(&ring)
    );

    // A cut at fed 0 of a call that also carries the `n`.
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = filter.feed(b"\x1b[6", DIMS).1;
    ring.extend_from_slice(&filter.feed_with_cuts(b"n", DIMS, &[0]).bytes);
    assert!(
        ring == [&b"\x1b[6"[..], CSI_CLOSING, b"n"].concat(),
        "{:?}",
        text(&ring)
    );
    assert_eq!(filter.csi_phase(), None);

    // The replayed ring equals the raw stream with the switch inline.
    let raw_before = b"\x1b[6\x1b[?1049h\x1b[?1049l";
    assert_replays_like_raw(&ring, raw_before, b"n", "cut then n");
}

// ── TS-5 (AC-3, FR3): the classification parity ──────────────────────────

/// Open CSI prefixes: a private marker, intermediates, long, saturating and
/// aliasing parameters, and C0 bytes inside.
fn parity_prefixes() -> Vec<Vec<u8>> {
    let mut prefixes: Vec<Vec<u8>> = [
        // Entry and parameters.
        &b"\x1b["[..],
        b"\x1b[0",
        b"\x1b[6",
        b"\x1b[5",
        b"\x1b[7",
        b"\x1b[;",
        b"\x1b[:",
        b"\x1b[6;",
        b"\x1b[6;5",
        b"\x1b[5;6",
        b"\x1b[;6",
        b"\x1b[6:",
        b"\x1b[12;34",
        b"\x1b[38:2:1:2:3",
        // A private marker.
        b"\x1b[?",
        b"\x1b[>",
        b"\x1b[<",
        b"\x1b[=",
        b"\x1b[?1",
        b"\x1b[>1",
        b"\x1b[=1",
        b"\x1b[<1",
        b"\x1b[?25",
        b"\x1b[?6",
        b"\x1b[>6",
        // Intermediates.
        b"\x1b[ ",
        b"\x1b[6 ",
        b"\x1b[ 6",
        b"\x1b[ 6$",
        b"\x1b[6$",
        b"\x1b[6!",
        b"\x1b[6 $",
        b"\x1b[?$",
        b"\x1b[>$",
        b"\x1b[?1$",
        b"\x1b[?1$$",
        b"\x1b[?1 $",
        b"\x1b[? $",
        b"\x1b[>1 ",
        b"\x1b[?1!",
        // A marker after a parameter or an intermediate cancels.
        b"\x1b[6?",
        b"\x1b[ ?",
        // C0 bytes execute inside the CSI.
        b"\x1b[\r",
        b"\x1b[6\r",
        b"\x1b[\r6",
        b"\x1b[?\r",
        b"\x1b[?1\x08$",
        b"abc\x1b[6",
        b"abc\x1b[1m\x1b[?1$",
    ]
    .iter()
    .map(|p| p.to_vec())
    .collect();
    // The first parameter: aliasing (mod 256), the clamp at 9999, and values
    // that saturate the accumulator.
    for value in [
        "1",
        "5",
        "6",
        "14",
        "16",
        "18",
        "255",
        "256",
        "257",
        "260",
        "261",
        "262",
        "517",
        "773",
        "9999",
        "10000",
        "10005",
        "10006",
        "65541",
        "4294967295",
        "4294967296",
        "4294967301",
        "99999999999999999999999999",
    ] {
        prefixes.push(format!("\x1b[{value}").into_bytes());
        prefixes.push(format!("\x1b[?{value}").into_bytes());
    }
    prefixes.push([b"\x1b[".to_vec(), b"1".repeat(100)].concat());
    prefixes.push([b"\x1b[".to_vec(), b"9".repeat(40)].concat());
    prefixes.push([b"\x1b[6".to_vec(), b";9".repeat(50)].concat());
    prefixes.push([b"\x1b[;".to_vec(), b"6".repeat(50)].concat());
    prefixes
}

/// AC-3 (FR3, TM-2, TS-5): for every open prefix and every next byte, the filter
/// writes CSI_CLOSING in place of that byte exactly when term_core answers the
/// raw stream. The prefix goes in one call, the byte in the next. A byte that
/// does not complete a query is written unchanged. `ESC` is held, DEL is DEL.
#[test]
fn strip_concat_the_classification_matches_term_core_over_every_prefix_and_next_byte() {
    let start = Instant::now();
    for prefix in parity_prefixes() {
        assert!(
            !answers(&prefix),
            "{:?}: the prefix itself answers",
            text(&prefix)
        );
        for byte in 0u8..=255 {
            let ctx = format!("prefix {:?} then {byte:#04x}", text(&prefix));
            let mut filter = ScrollbackWriteFilter::new();
            let first = filter.feed(&prefix, DIMS).1;
            assert!(
                first == prefix,
                "{ctx}: the prefix is written as it arrives"
            );
            let out = filter.feed(&[byte], DIMS).1;
            if byte == ESC {
                assert!(out.is_empty(), "{ctx}: a lone ESC is held");
                continue;
            }
            let stream = [&prefix[..], &[byte][..]].concat();
            if byte == DEL {
                assert!(out == [DEL], "{ctx}");
                continue;
            }
            let answered = answers(&stream);
            if answered {
                assert!(
                    out == CSI_CLOSING,
                    "{ctx}: term_core answers the raw stream, so the byte is written as DEL; \
                     written {:?}",
                    text(&out)
                );
            } else {
                assert!(
                    out == [byte],
                    "{ctx}: term_core does not answer, so the byte is written unchanged; \
                     written {:?}",
                    text(&out)
                );
            }
            if answered {
                let ring = [&first[..], &out[..]].concat();
                assert!(!answers(&ring), "{ctx}: replaying the ring answers");
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── TS-6 (AC-4, FR4): a lone ESC before a removed construct ──────────────

/// `(name, the raw bytes before the continuation, the continuation, the ring)`
/// for a lone `ESC` and an `ESC ESC` chain before each removed construct.
fn lone_esc_cases() -> Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)> {
    let mut cases = Vec::new();
    for target in TARGETS {
        for cont in [&b"[c"[..], b"[6n"] {
            for escs in [1usize, 2] {
                let lead = vec![ESC; escs];
                let before = [&lead[..], target.bytes].concat();
                let ring = [&lead[..], CSI_CLOSING, target.c0, cont].concat();
                cases.push((
                    format!("{escs} ESC + {} + {:?}", target.name, text(cont)),
                    before,
                    cont.to_vec(),
                    ring,
                ));
            }
        }
    }
    cases
}

/// AC-4 (FR4, NFR3, TM-1, TS-6): `ESC` and `ESC ESC` before each removed
/// construct and `[c` / `[6n` after it, in one call and split at every
/// position, including a lone trailing `ESC` held across calls (EC-11). The
/// neutralizing CSI_CLOSING precedes any re-emitted C0 byte; replay gives no
/// response and equals the raw-stream reference. After every call the carried
/// state is none or an open CSI that term_core agrees with, never an escape
/// state.
#[test]
fn strip_concat_a_lone_esc_before_a_removed_construct_is_neutralized() {
    for (name, before, cont, expected) in lone_esc_cases() {
        let fed = [&before[..], &cont[..]].concat();
        let ring = feed_checked(&[&fed[..]], &name);
        assert!(
            ring == expected,
            "{name}: one call: {:?}, expected {:?}",
            text(&ring),
            text(&expected)
        );
        assert_replays_like_raw(&ring, &before, &cont, &name);

        for split in 1..fed.len() {
            let ctx = format!("{name}, split at {split}");
            let (a, b) = fed.split_at(split);
            let ring = feed_checked(&[a, b], &ctx);
            // The split may write a query's final byte as DEL where the whole
            // call removes the query: the replay is what has to agree.
            assert_replays_like_raw(&ring, &before, &cont, &ctx);
            if !TARGETS
                .iter()
                .any(|t| t.is_query() && before.ends_with(t.bytes))
            {
                assert!(ring == expected, "{ctx}: {:?}", text(&ring));
            }
        }

        let bytes: Vec<&[u8]> = fed.chunks(1).collect();
        let ring = feed_checked(&bytes, &format!("{name}, byte by byte"));
        assert_replays_like_raw(&ring, &before, &cont, &format!("{name}, byte by byte"));
    }
}

/// EC-11 (FR4, TS-6): a lone `ESC` held across calls, then `ESC[6n[c`, is
/// resolved inside one strip pass once the chain settles.
#[test]
fn strip_concat_a_lone_esc_held_across_calls_is_resolved_in_one_pass() {
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = filter.feed(b"\x1b", DIMS).1;
    assert!(ring.is_empty());
    assert_eq!(filter.pending(), b"\x1b");
    assert_eq!(filter.csi_phase(), None);
    ring.extend_from_slice(&filter.feed(b"\x1b[6n[c", DIMS).1);
    assert!(
        ring == [&[ESC][..], CSI_CLOSING, b"[c"].concat(),
        "{:?}",
        text(&ring)
    );
    assert!(filter.pending().is_empty());
    assert_eq!(filter.csi_phase(), None);
    assert_replays_like_raw(&ring, b"\x1b\x1b[6n", b"[c", "ESC | ESC[6n[c");

    // The same with the ESC held after an open CSI: the CSI the filter carried
    // in is aborted by the held ESC once it is written.
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = filter.feed(b"\x1b[6", DIMS).1;
    ring.extend_from_slice(&filter.feed(b"\x1b", DIMS).1);
    ring.extend_from_slice(&filter.feed(b"\x1b[6n[c", DIMS).1);
    assert_eq!(filter.csi_phase(), None);
    assert_replays_like_raw(&ring, b"\x1b[6\x1b\x1b[6n", b"[c", "ESC[6 | ESC | ESC[6n[c");
}

// ── TS-9 (AC-7, NFR1, NFR2): budgets ─────────────────────────────────────

fn repeated(unit: &[u8], reps: usize) -> Vec<u8> {
    std::iter::repeat_n(unit.to_vec(), reps).flatten().collect()
}

/// AC-7 (NFR1, NFR2, TS-9): an open CSI and a strip target alternating 100k
/// times, a lone ESC and a strip target alternating 100k times and a 100k ESC
/// chain before each removed construct finish within the budget through the
/// strips and the write filter (in one call, which takes the overflow flush,
/// and in unit-aligned and unaligned reads), and write what the plan says.
#[test]
fn strip_concat_alternations_and_chains_finish_within_the_budget() {
    let start = Instant::now();
    let reps = 100_000usize;
    for target in TARGETS {
        for (shape, lead) in [("open CSI", &b"\x1b[6"[..]), ("lone ESC", &[ESC][..])] {
            let unit = [lead, target.bytes].concat();
            let input = repeated(&unit, reps);
            let expected_unit = [lead, CSI_CLOSING, target.c0].concat();
            let expected = repeated(&expected_unit, reps);
            let ctx = format!("{shape} + {}", target.name);

            assert!(
                strip_pty_output_for_scrollback_write(&input) == expected,
                "{ctx}: strip"
            );
            assert!(
                strip_replayable_rich_content(&input) == expected,
                "{ctx}: snapshot strip"
            );

            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed(&input, DIMS).1;
            assert!(out == expected, "{ctx}: one call (the overflow flush)");
            assert_eq!(filter.csi_phase(), None, "{ctx}");

            // Reads aligned to the unit never split a construct.
            let aligned = unit.len() * (65_536 / unit.len());
            let mut filter = ScrollbackWriteFilter::new();
            let mut out = Vec::new();
            for read in input.chunks(aligned) {
                out.extend_from_slice(&filter.feed(read, DIMS).1);
            }
            assert!(out == expected, "{ctx}: unit-aligned reads");

            // Reads of 64 KiB that split constructs: only the time is bound.
            let mut filter = ScrollbackWriteFilter::new();
            let mut out = Vec::new();
            for read in input.chunks(65_536) {
                out.extend_from_slice(&filter.feed(read, DIMS).1);
            }
            assert!(!out.is_empty(), "{ctx}: unaligned reads");
        }

        // A chain of ESC before the removed construct.
        let chain = vec![ESC; 100_000];
        let input = [&chain[..], target.bytes, b"x"].concat();
        let expected = [&chain[..], CSI_CLOSING, target.c0, b"x"].concat();
        let ctx = format!("ESC chain + {}", target.name);
        assert!(
            strip_pty_output_for_scrollback_write(&input) == expected,
            "{ctx}: strip"
        );
        let mut filter = ScrollbackWriteFilter::new();
        let out = filter.feed(&input, DIMS).1;
        assert!(out == expected, "{ctx}: one call");
        let mut filter = ScrollbackWriteFilter::new();
        let mut out = Vec::new();
        for read in input.chunks(16_384) {
            out.extend_from_slice(&filter.feed(read, DIMS).1);
        }
        assert!(
            out.starts_with(&chain[..chain.len() - 1]) && out.ends_with(b"x"),
            "{ctx}: reads"
        );
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-7 (NFR1, NFR2, TS-9): a 300k-byte CSI parameter run, fed whole and byte
/// by byte. The write filter never holds a CSI byte, and a final byte that
/// completes an answered query is written as CSI_CLOSING however long the run.
#[test]
fn strip_concat_a_long_csi_parameter_run_is_never_held() {
    let start = Instant::now();
    let digits: Vec<u8> = (0..300_000usize).map(|i| b'0' + (i % 10) as u8).collect();
    let head = [&b"\x1b["[..], &digits[..]].concat();

    // Whole: written as it arrives, ending in an open CSI.
    let mut filter = ScrollbackWriteFilter::new();
    let out = filter.feed(&head, DIMS).1;
    assert!(out == head);
    assert!(filter.pending().is_empty());
    assert_eq!(filter.csi_phase(), Some(CsiPhase::Param));
    // The completing final byte, in a later call: DA1 is answered whatever the
    // parameter.
    assert!(filter.feed(b"c", DIMS).1 == CSI_CLOSING);
    assert_eq!(filter.csi_phase(), None);

    // Whole with the final byte: the query is removed.
    let mut filter = ScrollbackWriteFilter::new();
    let out = filter.feed(&[&head[..], b"c"].concat(), DIMS).1;
    assert!(out.is_empty());

    // Byte by byte.
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = filter.feed(b"\x1b[", DIMS).1;
    for (idx, byte) in digits.iter().enumerate() {
        ring.extend_from_slice(&filter.feed(&[*byte], DIMS).1);
        assert_eq!(filter.pending_len(), 0, "no CSI byte is held (byte {idx})");
    }
    assert!(ring == head, "every byte is written as it arrives");
    assert_eq!(filter.csi_phase(), Some(CsiPhase::Param));
    // The accumulator saturates; the clamped parameter does not alias DSR.
    assert!(filter.feed(b"n", DIMS).1 == b"n");
    let mut filter2 = ScrollbackWriteFilter::new();
    filter2.feed(&head, DIMS);
    assert!(filter2.feed(b"t", DIMS).1 == b"t");
    assert!(filter.pending().is_empty());
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// EC-7 (FR1, FR7, TS-9): the overflow flush strips the flushed run with the
/// closing in place, and a cut that follows writes no second closing.
#[test]
fn strip_concat_the_overflow_flush_writes_the_strips_closing_in_place() {
    let start = Instant::now();
    let held = osc_held_at_the_cap();
    for target in TARGETS {
        for (shape, lead) in [("open CSI", &b"\x1b[6"[..]), ("lone ESC", &[ESC][..])] {
            let ctx = format!("{shape} + {}", target.name);
            // The continuation of the held OSC takes the run past the cap and
            // ends in the head and the construct.
            let tail: Vec<u8> = [&b"pppppppppp"[..], b"\x07", b"abc", lead, target.bytes].concat();
            let whole = [&held[..], &tail[..]].concat();
            let expected_suffix = [&b"abc"[..], lead, CSI_CLOSING, target.c0].concat();
            let stripped = strip_pty_output_for_scrollback_write(&whole);
            assert!(stripped.ends_with(&expected_suffix), "{ctx}: the strip");

            let mut filter = ScrollbackWriteFilter::new();
            assert!(filter.feed(&held, DIMS).1.is_empty());
            let outcome = filter.feed_with_cuts(&tail, DIMS, &[]);
            assert!(outcome.carried.is_none(), "{ctx}");
            assert!(
                outcome.bytes == stripped,
                "{ctx}: the flush writes the strip"
            );
            assert_eq!(filter.csi_phase(), None, "{ctx}: the closing left no CSI");
            assert!(filter.pending().is_empty(), "{ctx}");
            assert!(!filter.awaiting_designator(), "{ctx}");

            // A cut at the end of the flushing call writes no second closing.
            let mut filter = ScrollbackWriteFilter::new();
            assert!(filter.feed(&held, DIMS).1.is_empty());
            let outcome = filter.feed_with_cuts(&tail, DIMS, &[tail.len()]);
            assert!(outcome.bytes == stripped, "{ctx}: flush, then a cut");
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}
