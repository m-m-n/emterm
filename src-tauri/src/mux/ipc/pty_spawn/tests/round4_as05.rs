//! mux-suppressed-output-round4-fixes task0003 (FR2, finding
//! `989ec5c588abce06`): the as-05 fallback of the client-parity scan reads a
//! suppressed chunk two ways and keeps only what both readings report.
//!
//! The fallback applies when the reader's retention window is full, holds no
//! decidable ESC and ends in `ESC (` / `ESC )`. The chunk's first byte may
//! then have been consumed by the client as the designator (reading a), or
//! parsed from ground (reading b); the window alone cannot tell which. The
//! scan keeps an item only when both readings report the same range and kind,
//! and a tail only when both report the same start. The rule can only remove
//! results, so it can cause misses but never a fabricated query.
//!
//! Oracle convention (IMPLEMENTATION.md "Reader-level oracle convention"):
//! responses are always compared with the raw-stream reference; the screen is
//! compared when the stand-in snapshot itself reproduces the reference's
//! screen for its prefix (`r2_snapshot_reproduces_prefix`).

use std::ops::Range;

use super::*;
use crate::mux::ipc::pty_spawn::client_parity_scan::{
    RETAINED_WINDOW_BYTES, ScanItemKind, ScanOutcome, scan, scan_with_reading_count,
};

const ESC: u8 = 0x1b;

/// The suppressed chunk of the earlier as-05 tests: an OSC 11 color query.
const COLOR_QUERY: &[u8] = b"\x1b]11;?\x07";

/// `pairs` consecutive `ESC <intro>`.
fn esc_chain(intro: u8, pairs: usize) -> Vec<u8> {
    [ESC, intro].repeat(pairs)
}

/// `pairs` consecutive `ESC <intro>`, then `rest`.
fn chain_then(intro: u8, pairs: usize, rest: &[u8]) -> Vec<u8> {
    let mut bytes = esc_chain(intro, pairs);
    bytes.extend_from_slice(rest);
    bytes
}

/// A full window in ground state (no ESC at all): the scan starts at the
/// chunk and nothing is undecided.
fn ground_window() -> Vec<u8> {
    vec![b'x'; RETAINED_WINDOW_BYTES]
}

/// The retention window after a long `ESC <intro>` chain: exactly
/// [`RETAINED_WINDOW_BYTES`] bytes, no decidable ESC, ending in `ESC <intro>`.
fn fallback_window(intro: u8) -> Vec<u8> {
    esc_chain(intro, RETAINED_WINDOW_BYTES / 2)
}

/// Every shape of full window that satisfies the as-05 fallback condition.
fn fallback_windows() -> Vec<(&'static str, Vec<u8>)> {
    let mut mixed = Vec::new();
    for i in 0..RETAINED_WINDOW_BYTES / 2 {
        mixed.extend_from_slice(&[ESC, if i % 2 == 0 { b'(' } else { b')' }]);
    }
    // `(` at offset 0, then `ESC (` pairs (one byte over the retention size:
    // at exactly that size such a window cannot end in `ESC (`).
    let mut paren_led = vec![b'('];
    paren_led.extend_from_slice(&esc_chain(b'(', RETAINED_WINDOW_BYTES / 2));
    vec![
        ("ESC ( chain", fallback_window(b'(')),
        ("ESC ) chain", fallback_window(b')')),
        ("mixed chain", mixed),
        ("( at offset 0, ESC ( chain", paren_led),
    ]
}

/// A compact, comparable rendering of everything a scan reports.
fn render(outcome: &ScanOutcome) -> String {
    let items: Vec<String> = outcome
        .items
        .iter()
        .map(|item| format!("{:?} {}..{}", item.kind, item.range.start, item.range.end))
        .collect();
    format!(
        "boundary={} items=[{}] tail={:?} strip={}",
        outcome.boundary,
        items.join(", "),
        outcome.tail,
        outcome.tail_strip_c0
    )
}

/// The items of `outcome` as `(kind, range)` in the chunk's own coordinates.
fn items_in_chunk(outcome: &ScanOutcome) -> Vec<(ScanItemKind, Range<usize>)> {
    outcome
        .items
        .iter()
        .map(|item| {
            (
                item.kind,
                item.range.start.saturating_sub(outcome.boundary)
                    ..item.range.end.saturating_sub(outcome.boundary),
            )
        })
        .collect()
}

/// The tail of `outcome` in the chunk's own coordinates.
fn tail_in_chunk(outcome: &ScanOutcome) -> Option<Range<usize>> {
    outcome.tail.as_ref().map(|tail| {
        tail.start.saturating_sub(outcome.boundary)..tail.end.saturating_sub(outcome.boundary)
    })
}

/// Run `chunks` (earlier reads, then the suppressed read, then any later
/// reads) through the production reader with read `1` suppressed, and compare
/// the client's responses with the raw-stream reference. The screen is
/// compared too when the stand-in snapshot reproduces the reference's screen
/// for its own prefix.
fn run_and_compare(chunks: &[Vec<u8>], exact_responses: bool, ctx: &str) -> SuppressedRun {
    let run = run_reader_with_suppressed_reads(chunks, &[1]);
    assert_r2_responses_match(&run, chunks, exact_responses, ctx);
    let stream = chunks.concat();
    let prefix_len = chunks[0].len() + chunks[1].len();
    if r2_snapshot_reproduces_prefix(&run, &stream, prefix_len) {
        assert_r2_screen_matches(&run, chunks, ctx);
    }
    run
}

// ---- AC-1 (FR2, TM-1, registry) ----

/// AC-1 (FR2, TM-1; registry `989ec5c588abce06`): 129 consecutive `ESC <intro>`
/// in earlier reads leave the retention window full, with no decidable ESC and
/// ending in `ESC <intro>`; the stream is awaiting a designator, so the
/// suppressed chunk's first byte is consumed as one. A designator chain
/// inside the chunk (`ESC ( ESC (`) then turns the ESC that opens `ESC ]` into
/// a designator too, so the client never saw a query or a CSI there. Reading
/// the chunk from ground, as the scan did before, finds the query and the open
/// CSI; the intersection with the other reading removes both.
///
/// - `ESC ( ESC ( ESC ] 11 ; ? BEL`: no item, no replacement query, no
///   response;
/// - `ESC ( ESC ( ESC [`: no tail, and a following read `6n` produces no DSR
///   response;
/// - the same behind a trailing `ESC )` (and with `ESC )` chains inside the
///   chunk).
///
/// Driven through the production reader with the suppressed read covered by a
/// stand-in snapshot; the client is compared with the raw-stream reference.
#[test]
fn round4_989ec5c5_as05_fallback_never_fabricates_after_a_designator_chain_inside_the_chunk() {
    for window_intro in [b'(', b')'] {
        for chunk_intro in [b'(', b')'] {
            let label = format!(
                "window ESC {} chain, chunk ESC {} chain",
                window_intro as char, chunk_intro as char
            );

            let query_chunk = chain_then(chunk_intro, 2, COLOR_QUERY);
            let open_csi_chunk = chain_then(chunk_intro, 2, b"\x1b[");

            // The query chunk.
            let chain = esc_chain(window_intro, RETAINED_WINDOW_BYTES / 2 + 1);
            let chunks = vec![chain.clone(), query_chunk.clone()];
            let run = run_and_compare(&chunks, true, &format!("{label}: suppressed query"));
            assert_eq!(
                run.pty_output(),
                vec![chain.clone()],
                "{label}: only the earlier read reaches the client; nothing is sent for the suppressed chunk"
            );
            assert_eq!(
                r2_count(&run.pty_output_bytes(), COLOR_QUERY),
                0,
                "{label}: the query must not be fabricated"
            );
            let (_client, client_responses) = r2_client_view(&run);
            assert!(
                client_responses.is_empty(),
                "{label}: the client produces no response: {client_responses:?}"
            );
            let (_reference, reference_responses) = r2_reference(&chunks);
            assert!(
                reference_responses.is_empty(),
                "{label}: the raw stream's reference produces none either: {reference_responses:?}"
            );

            // The open CSI, completed by the following read.
            let chunks = vec![chain.clone(), open_csi_chunk.clone(), b"6n".to_vec()];
            let run = run_and_compare(&chunks, true, &format!("{label}: open CSI then 6n"));
            assert_eq!(
                run.pty_output(),
                vec![chain, b"6n".to_vec()],
                "{label}: no tail is re-delivered for the suppressed chunk"
            );
            let (_client, client_responses) = r2_client_view(&run);
            assert!(
                client_responses.is_empty(),
                "{label}: the following `6n` produces no DSR response: {client_responses:?}"
            );
            let (_reference, reference_responses) = r2_reference(&chunks);
            assert!(
                reference_responses.is_empty(),
                "{label}: the raw stream's reference produces none either: {reference_responses:?}"
            );
        }
    }
}

/// AC-1 (FR2, TM-1), at the scan: in every shape of full fallback window the
/// scan reports no item and no tail for the two chunks above, while the same
/// chunks behind a ground window (the controls) do report the query and the
/// open CSI, so the intersection is what removes them.
#[test]
fn round4_as05_scan_reports_nothing_behind_a_designator_chain_in_the_chunk() {
    for (window_label, window) in fallback_windows() {
        for chunk_intro in [b'(', b')'] {
            let label = format!("{window_label}, chunk ESC {} chain", chunk_intro as char);
            let query_chunk = chain_then(chunk_intro, 2, COLOR_QUERY);
            let open_csi_chunk = chain_then(chunk_intro, 2, b"\x1b[");
            let query_control = scan(&ground_window(), &query_chunk, &[]);
            assert_eq!(
                items_in_chunk(&query_control),
                vec![(ScanItemKind::ColorQuery, 4..11)],
                "{label}: the control scan from ground reads the query"
            );
            let csi_control = scan(&ground_window(), &open_csi_chunk, &[]);
            assert_eq!(
                tail_in_chunk(&csi_control),
                Some(4..6),
                "{label}: the control scan from ground reads the open CSI"
            );
            assert!(
                csi_control.tail_strip_c0,
                "{label}: the control tail strips C0"
            );

            let outcome = scan(&window, &query_chunk, &[]);
            assert_eq!(
                outcome.boundary, 0,
                "{label}: the fallback starts at the chunk"
            );
            assert!(
                outcome.items.is_empty(),
                "{label}: no item may be reported for the chunk, got {:?}",
                outcome.items
            );
            assert_eq!(outcome.tail, None, "{label}: no tail for the query chunk");
            let outcome = scan(&window, &open_csi_chunk, &[]);
            assert!(
                outcome.items.is_empty(),
                "{label}: no item for the open CSI"
            );
            assert_eq!(outcome.tail, None, "{label}: no tail for the open CSI");
            assert!(
                !outcome.tail_strip_c0,
                "{label}: no C0-strip flag without a tail"
            );
        }
    }
}

/// AC-1 (FR2, accepted miss): the same streams with an even chain (128 pairs).
/// The window is then in ground state, so the reference does answer, but the
/// window alone cannot tell the two parities apart; the intersection misses.
/// Only fabrication is asserted: whatever the client answers equals what the
/// reference answered.
#[test]
fn round4_as05_even_chain_never_answers_more_than_the_reference() {
    for intro in [b'(', b')'] {
        for tail_read in [None, Some(b"6n".as_slice())] {
            let first = chain_then(intro, 2, COLOR_QUERY);
            let second = chain_then(intro, 2, b"\x1b[");
            let chain = esc_chain(intro, RETAINED_WINDOW_BYTES / 2);
            let mut chunks = vec![chain, if tail_read.is_some() { second } else { first }];
            if let Some(read) = tail_read {
                chunks.push(read.to_vec());
            }
            let run = run_reader_with_suppressed_reads(&chunks, &[1]);
            let (_client, client_responses) = r2_client_view(&run);
            let (_reference, reference_responses) = r2_reference(&chunks);
            assert!(
                client_responses.is_empty() || client_responses == reference_responses,
                "ESC {} chain, tail read {tail_read:?}: no response may be fabricated: client {client_responses:?}, reference {reference_responses:?}",
                intro as char
            );
            assert!(
                r2_count(&run.pty_output_bytes(), COLOR_QUERY) <= 1,
                "the query is delivered at most once"
            );
        }
    }
}

// ---- AC-2 (FR2, SPEC AC-5) ----

/// AC-2 (FR2, SPEC AC-5): items and tails that both readings report
/// identically are still delivered. Each case runs in every fallback window
/// and equals the same chunk behind a ground window (the control).
#[test]
fn round4_as05_items_and_tails_both_readings_report_identically_are_kept() {
    // (label, chunk, expected items, expected tail, expected strip flag)
    type Case = (
        &'static str,
        Vec<u8>,
        Vec<(ScanItemKind, Range<usize>)>,
        Option<Range<usize>>,
        bool,
    );
    let cases: Vec<Case> = vec![
        (
            "plain text byte, then a color query",
            b"x\x1b]11;?\x07".to_vec(),
            vec![(ScanItemKind::ColorQuery, 1..8)],
            None,
            false,
        ),
        (
            "two plain bytes, then a CSI query",
            b"ab\x1b[6n".to_vec(),
            vec![(ScanItemKind::CsiQuery, 2..6)],
            None,
            false,
        ),
        (
            "designator pair, then a CSI query",
            b"\x1b(A\x1b[6n".to_vec(),
            vec![(ScanItemKind::CsiQuery, 3..7)],
            None,
            false,
        ),
        (
            "ESC ESC, then a color query",
            b"\x1b\x1b]11;?\x07".to_vec(),
            vec![(ScanItemKind::ColorQuery, 1..8)],
            None,
            false,
        ),
        (
            "plain text byte, then a viewer launch",
            b"x\x1b]777;emterm;markdown;begin;id=r4\x07".to_vec(),
            vec![(ScanItemKind::ViewerLaunch, 1..35)],
            None,
            false,
        ),
        (
            "OSC title, then a CSI query",
            b"\x1b]2;x\x07\x1b[6n".to_vec(),
            vec![(ScanItemKind::CsiQuery, 6..10)],
            None,
            false,
        ),
        (
            "plain text byte, a CSI query, then an incomplete CSI",
            b"x\x1b[6n\x1b[3".to_vec(),
            vec![(ScanItemKind::CsiQuery, 1..5)],
            Some(5..8),
            true,
        ),
        (
            "designator pair, then an incomplete CSI",
            b"\x1b(A\x1b[3".to_vec(),
            vec![],
            Some(3..6),
            true,
        ),
        (
            "plain text byte, then an incomplete OSC",
            b"x\x1b]11;?".to_vec(),
            vec![],
            Some(1..7),
            false,
        ),
        (
            "an incomplete UTF-8 character at the chunk start",
            vec![0xe4, 0xb8],
            vec![],
            Some(0..2),
            false,
        ),
        (
            "a single UTF-8 lead byte",
            vec![0xe4],
            vec![],
            Some(0..1),
            false,
        ),
        (
            "an incomplete UTF-8 character after text",
            vec![b'a', 0xe4],
            vec![],
            Some(1..2),
            false,
        ),
        (
            "an incomplete 4-byte UTF-8 character",
            vec![0xf0, 0x9f, 0x98],
            vec![],
            Some(0..3),
            false,
        ),
    ];
    for (label, chunk, expected_items, expected_tail, expected_strip) in cases {
        let control = scan(&ground_window(), &chunk, &[]);
        assert_eq!(
            items_in_chunk(&control),
            expected_items,
            "{label}: control items"
        );
        assert_eq!(
            tail_in_chunk(&control),
            expected_tail,
            "{label}: control tail"
        );
        assert_eq!(
            control.tail_strip_c0, expected_strip,
            "{label}: control C0-strip flag"
        );

        for (window_label, window) in fallback_windows() {
            let ctx = format!("{window_label} / {label}");
            let outcome = scan(&window, &chunk, &[]);
            assert_eq!(
                outcome.boundary, 0,
                "{ctx}: the fallback starts at the chunk"
            );
            assert_eq!(items_in_chunk(&outcome), expected_items, "{ctx}: items");
            assert_eq!(tail_in_chunk(&outcome), expected_tail, "{ctx}: tail");
            assert_eq!(
                outcome.tail_strip_c0, expected_strip,
                "{ctx}: C0-strip flag"
            );
        }
    }
}

/// AC-2 (FR2), through the reader: the retention window is full of `ESC (`
/// and the suppressed chunk's first byte is plain text, followed by a complete
/// color query. Both readings find the query, so it is delivered once and the
/// client answers exactly as the reference does. The control is the same chunk
/// behind a ground window.
#[test]
fn round4_as05_plain_first_byte_then_a_color_query_is_still_delivered_once() {
    let chunk = [b"x".as_slice(), COLOR_QUERY].concat();
    for (label, first_read) in [
        (
            "ESC ( chain",
            esc_chain(b'(', RETAINED_WINDOW_BYTES / 2 + 1),
        ),
        (
            "ESC ) chain",
            esc_chain(b')', RETAINED_WINDOW_BYTES / 2 + 1),
        ),
        (
            "ground window (control)",
            vec![b'y'; RETAINED_WINDOW_BYTES + 2],
        ),
    ] {
        let chunks = vec![first_read, chunk.clone()];
        let run = run_and_compare(&chunks, true, label);
        assert_eq!(
            r2_count(&run.pty_output_bytes(), COLOR_QUERY),
            1,
            "{label}: the query is re-delivered once"
        );
        let (_client, client_responses) = r2_client_view(&run);
        let (_reference, reference_responses) = r2_reference(&chunks);
        assert_eq!(client_responses, reference_responses, "{label}: responses");
    }
}

// ---- AC-3 (FR2) ----

/// Windows outside the fallback condition: below the retention size, with a
/// decidable ESC, with a trailing `(` / `)` that no ESC precedes, and with a
/// trailing UTF-8 partial.
fn corpus_windows() -> Vec<(&'static str, Vec<u8>)> {
    let n = RETAINED_WINDOW_BYTES;

    let mut short_chain = vec![b'x'; n - 5];
    short_chain.extend_from_slice(&[ESC, b'(', ESC, b'(']);

    let mut short_awaiting = vec![b'x'; 100];
    short_awaiting.extend_from_slice(&[ESC, b'(']);

    // Full windows holding a decidable ESC early, ending in `ESC ( ESC (` /
    // a single `ESC (`.
    let mut decidable_chain_end = vec![b'x', ESC, b'[', b'0', b'm'];
    decidable_chain_end.resize(n - 4, b'y');
    decidable_chain_end.extend_from_slice(&[ESC, b'(', ESC, b'(']);
    let mut decidable_awaiting = vec![b'x', ESC, b'[', b'0', b'm'];
    decidable_awaiting.resize(n - 2, b'y');
    decidable_awaiting.extend_from_slice(&[ESC, b'(']);

    // A full window whose decidable ESC opens an OSC the chunk can complete.
    let mut decidable_open_osc = vec![b'x', ESC, b']', b'1', b'1', b';'];
    decidable_open_osc.resize(n, b' ');

    // A trailing `(` / `)` that is not preceded by ESC.
    let mut paren_after_text = vec![b'x'; n - 1];
    paren_after_text.push(b'(');
    let mut rparen_after_text = vec![b'x'; n - 1];
    rparen_after_text.push(b')');

    // A full window ending in a UTF-8 partial.
    let mut utf8_partial = vec![b'x'; n - 2];
    utf8_partial.extend_from_slice(&[0xe4, 0xb8]);

    vec![
        ("empty window", vec![]),
        ("short ground window", vec![b'x'; 100]),
        ("short window ending in ESC (", short_awaiting),
        ("short window ending in ESC ( ESC (", short_chain),
        ("full ground window", vec![b'x'; n]),
        (
            "full window, decidable ESC, ends ESC ( ESC (",
            decidable_chain_end,
        ),
        ("full window, decidable ESC, ends ESC (", decidable_awaiting),
        ("full window, decidable open OSC", decidable_open_osc),
        ("full window, trailing ( after text", paren_after_text),
        ("full window, trailing ) after text", rparen_after_text),
        ("full window of ( only", vec![b'('; n]),
        ("full window, trailing UTF-8 partial", utf8_partial),
    ]
}

fn corpus_chunks() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("empty chunk", vec![]),
        ("OSC 11 query", b"\x1b]11;?\x07".to_vec()),
        ("CSI 6n query", b"\x1b[6n".to_vec()),
        ("text then OSC 11 query", b"x\x1b]11;?\x07".to_vec()),
        ("designator pair then CSI query", b"\x1b(A\x1b[6n".to_vec()),
        ("OSC title then CSI query", b"\x1b]2;x\x07\x1b[6n".to_vec()),
        (
            "OSC 777 markdown launch",
            b"\x1b]777;emterm;markdown;begin;id=r4\x07".to_vec(),
        ),
        ("incomplete CSI", b"\x1b[3".to_vec()),
        ("incomplete OSC", b"\x1b]11;?".to_vec()),
        ("UTF-8 partial", vec![0xe4, 0xb8]),
        ("lone ESC", vec![ESC]),
        ("ESC ESC then OSC 11 query", b"\x1b\x1b]11;?\x07".to_vec()),
        ("completing bytes of an open OSC", b"?\x07".to_vec()),
        (
            "ESC ( chain then OSC 11 query",
            b"\x1b(\x1b(\x1b]11;?\x07".to_vec(),
        ),
        ("ESC ( chain then open CSI", b"\x1b(\x1b(\x1b[".to_vec()),
    ]
}

/// What the scan of the parent revision (before the two-reading
/// intersection) reported for every window x chunk pair of the corpus, in
/// corpus order: `window|chunk|rendering`. Captured from that revision
/// and fixed here; not derived from any re-implementation of the old rule.
const PRE_CHANGE_CORPUS: &[&str] = &[
    "empty window|empty chunk|boundary=0 items=[] tail=None strip=false",
    "empty window|OSC 11 query|boundary=0 items=[ColorQuery 0..7] tail=None strip=false",
    "empty window|CSI 6n query|boundary=0 items=[CsiQuery 0..4] tail=None strip=false",
    "empty window|text then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "empty window|designator pair then CSI query|boundary=0 items=[CsiQuery 3..7] tail=None strip=false",
    "empty window|OSC title then CSI query|boundary=0 items=[CsiQuery 6..10] tail=None strip=false",
    "empty window|OSC 777 markdown launch|boundary=0 items=[ViewerLaunch 0..34] tail=None strip=false",
    "empty window|incomplete CSI|boundary=0 items=[] tail=Some(0..3) strip=true",
    "empty window|incomplete OSC|boundary=0 items=[] tail=Some(0..6) strip=false",
    "empty window|UTF-8 partial|boundary=0 items=[] tail=Some(0..2) strip=false",
    "empty window|lone ESC|boundary=0 items=[] tail=Some(0..1) strip=false",
    "empty window|ESC ESC then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "empty window|completing bytes of an open OSC|boundary=0 items=[] tail=None strip=false",
    "empty window|ESC ( chain then OSC 11 query|boundary=0 items=[ColorQuery 4..11] tail=None strip=false",
    "empty window|ESC ( chain then open CSI|boundary=0 items=[] tail=Some(4..6) strip=true",
    "short ground window|empty chunk|boundary=100 items=[] tail=None strip=false",
    "short ground window|OSC 11 query|boundary=100 items=[ColorQuery 100..107] tail=None strip=false",
    "short ground window|CSI 6n query|boundary=100 items=[CsiQuery 100..104] tail=None strip=false",
    "short ground window|text then OSC 11 query|boundary=100 items=[ColorQuery 101..108] tail=None strip=false",
    "short ground window|designator pair then CSI query|boundary=100 items=[CsiQuery 103..107] tail=None strip=false",
    "short ground window|OSC title then CSI query|boundary=100 items=[CsiQuery 106..110] tail=None strip=false",
    "short ground window|OSC 777 markdown launch|boundary=100 items=[ViewerLaunch 100..134] tail=None strip=false",
    "short ground window|incomplete CSI|boundary=100 items=[] tail=Some(100..103) strip=true",
    "short ground window|incomplete OSC|boundary=100 items=[] tail=Some(100..106) strip=false",
    "short ground window|UTF-8 partial|boundary=100 items=[] tail=Some(100..102) strip=false",
    "short ground window|lone ESC|boundary=100 items=[] tail=Some(100..101) strip=false",
    "short ground window|ESC ESC then OSC 11 query|boundary=100 items=[ColorQuery 101..108] tail=None strip=false",
    "short ground window|completing bytes of an open OSC|boundary=100 items=[] tail=None strip=false",
    "short ground window|ESC ( chain then OSC 11 query|boundary=100 items=[ColorQuery 104..111] tail=None strip=false",
    "short ground window|ESC ( chain then open CSI|boundary=100 items=[] tail=Some(104..106) strip=true",
    "short window ending in ESC (|empty chunk|boundary=102 items=[] tail=Some(100..102) strip=false",
    "short window ending in ESC (|OSC 11 query|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|CSI 6n query|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|text then OSC 11 query|boundary=102 items=[ColorQuery 103..110] tail=None strip=false",
    "short window ending in ESC (|designator pair then CSI query|boundary=102 items=[CsiQuery 105..109] tail=None strip=false",
    "short window ending in ESC (|OSC title then CSI query|boundary=102 items=[CsiQuery 108..112] tail=None strip=false",
    "short window ending in ESC (|OSC 777 markdown launch|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|incomplete CSI|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|incomplete OSC|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|UTF-8 partial|boundary=102 items=[] tail=Some(102..104) strip=false",
    "short window ending in ESC (|lone ESC|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|ESC ESC then OSC 11 query|boundary=102 items=[ColorQuery 103..110] tail=None strip=false",
    "short window ending in ESC (|completing bytes of an open OSC|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|ESC ( chain then OSC 11 query|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC (|ESC ( chain then open CSI|boundary=102 items=[] tail=None strip=false",
    "short window ending in ESC ( ESC (|empty chunk|boundary=255 items=[] tail=None strip=false",
    "short window ending in ESC ( ESC (|OSC 11 query|boundary=255 items=[ColorQuery 255..262] tail=None strip=false",
    "short window ending in ESC ( ESC (|CSI 6n query|boundary=255 items=[CsiQuery 255..259] tail=None strip=false",
    "short window ending in ESC ( ESC (|text then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "short window ending in ESC ( ESC (|designator pair then CSI query|boundary=255 items=[CsiQuery 258..262] tail=None strip=false",
    "short window ending in ESC ( ESC (|OSC title then CSI query|boundary=255 items=[CsiQuery 261..265] tail=None strip=false",
    "short window ending in ESC ( ESC (|OSC 777 markdown launch|boundary=255 items=[ViewerLaunch 255..289] tail=None strip=false",
    "short window ending in ESC ( ESC (|incomplete CSI|boundary=255 items=[] tail=Some(255..258) strip=true",
    "short window ending in ESC ( ESC (|incomplete OSC|boundary=255 items=[] tail=Some(255..261) strip=false",
    "short window ending in ESC ( ESC (|UTF-8 partial|boundary=255 items=[] tail=Some(255..257) strip=false",
    "short window ending in ESC ( ESC (|lone ESC|boundary=255 items=[] tail=Some(255..256) strip=false",
    "short window ending in ESC ( ESC (|ESC ESC then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "short window ending in ESC ( ESC (|completing bytes of an open OSC|boundary=255 items=[] tail=None strip=false",
    "short window ending in ESC ( ESC (|ESC ( chain then OSC 11 query|boundary=255 items=[ColorQuery 259..266] tail=None strip=false",
    "short window ending in ESC ( ESC (|ESC ( chain then open CSI|boundary=255 items=[] tail=Some(259..261) strip=true",
    "full ground window|empty chunk|boundary=0 items=[] tail=None strip=false",
    "full ground window|OSC 11 query|boundary=0 items=[ColorQuery 0..7] tail=None strip=false",
    "full ground window|CSI 6n query|boundary=0 items=[CsiQuery 0..4] tail=None strip=false",
    "full ground window|text then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full ground window|designator pair then CSI query|boundary=0 items=[CsiQuery 3..7] tail=None strip=false",
    "full ground window|OSC title then CSI query|boundary=0 items=[CsiQuery 6..10] tail=None strip=false",
    "full ground window|OSC 777 markdown launch|boundary=0 items=[ViewerLaunch 0..34] tail=None strip=false",
    "full ground window|incomplete CSI|boundary=0 items=[] tail=Some(0..3) strip=true",
    "full ground window|incomplete OSC|boundary=0 items=[] tail=Some(0..6) strip=false",
    "full ground window|UTF-8 partial|boundary=0 items=[] tail=Some(0..2) strip=false",
    "full ground window|lone ESC|boundary=0 items=[] tail=Some(0..1) strip=false",
    "full ground window|ESC ESC then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full ground window|completing bytes of an open OSC|boundary=0 items=[] tail=None strip=false",
    "full ground window|ESC ( chain then OSC 11 query|boundary=0 items=[ColorQuery 4..11] tail=None strip=false",
    "full ground window|ESC ( chain then open CSI|boundary=0 items=[] tail=Some(4..6) strip=true",
    "full window, decidable ESC, ends ESC ( ESC (|empty chunk|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|OSC 11 query|boundary=255 items=[ColorQuery 255..262] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|CSI 6n query|boundary=255 items=[CsiQuery 255..259] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|text then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|designator pair then CSI query|boundary=255 items=[CsiQuery 258..262] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|OSC title then CSI query|boundary=255 items=[CsiQuery 261..265] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|OSC 777 markdown launch|boundary=255 items=[ViewerLaunch 255..289] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|incomplete CSI|boundary=255 items=[] tail=Some(255..258) strip=true",
    "full window, decidable ESC, ends ESC ( ESC (|incomplete OSC|boundary=255 items=[] tail=Some(255..261) strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|UTF-8 partial|boundary=255 items=[] tail=Some(255..257) strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|lone ESC|boundary=255 items=[] tail=Some(255..256) strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|ESC ESC then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|completing bytes of an open OSC|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|ESC ( chain then OSC 11 query|boundary=255 items=[ColorQuery 259..266] tail=None strip=false",
    "full window, decidable ESC, ends ESC ( ESC (|ESC ( chain then open CSI|boundary=255 items=[] tail=Some(259..261) strip=true",
    "full window, decidable ESC, ends ESC (|empty chunk|boundary=255 items=[] tail=Some(253..255) strip=false",
    "full window, decidable ESC, ends ESC (|OSC 11 query|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|CSI 6n query|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|text then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|designator pair then CSI query|boundary=255 items=[CsiQuery 258..262] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|OSC title then CSI query|boundary=255 items=[CsiQuery 261..265] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|OSC 777 markdown launch|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|incomplete CSI|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|incomplete OSC|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|UTF-8 partial|boundary=255 items=[] tail=Some(255..257) strip=false",
    "full window, decidable ESC, ends ESC (|lone ESC|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|ESC ESC then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|completing bytes of an open OSC|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|ESC ( chain then OSC 11 query|boundary=255 items=[] tail=None strip=false",
    "full window, decidable ESC, ends ESC (|ESC ( chain then open CSI|boundary=255 items=[] tail=None strip=false",
    "full window, decidable open OSC|empty chunk|boundary=255 items=[] tail=Some(0..255) strip=false",
    "full window, decidable open OSC|OSC 11 query|boundary=255 items=[ColorQuery 255..262] tail=None strip=false",
    "full window, decidable open OSC|CSI 6n query|boundary=255 items=[CsiQuery 255..259] tail=None strip=false",
    "full window, decidable open OSC|text then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "full window, decidable open OSC|designator pair then CSI query|boundary=255 items=[CsiQuery 258..262] tail=None strip=false",
    "full window, decidable open OSC|OSC title then CSI query|boundary=255 items=[CsiQuery 261..265] tail=None strip=false",
    "full window, decidable open OSC|OSC 777 markdown launch|boundary=255 items=[ViewerLaunch 255..289] tail=None strip=false",
    "full window, decidable open OSC|incomplete CSI|boundary=255 items=[] tail=Some(255..258) strip=true",
    "full window, decidable open OSC|incomplete OSC|boundary=255 items=[] tail=Some(255..261) strip=false",
    "full window, decidable open OSC|UTF-8 partial|boundary=255 items=[] tail=Some(0..257) strip=false",
    "full window, decidable open OSC|lone ESC|boundary=255 items=[] tail=Some(0..256) strip=false",
    "full window, decidable open OSC|ESC ESC then OSC 11 query|boundary=255 items=[ColorQuery 256..263] tail=None strip=false",
    "full window, decidable open OSC|completing bytes of an open OSC|boundary=255 items=[ColorQuery 0..257] tail=None strip=false",
    "full window, decidable open OSC|ESC ( chain then OSC 11 query|boundary=255 items=[ColorQuery 259..266] tail=None strip=false",
    "full window, decidable open OSC|ESC ( chain then open CSI|boundary=255 items=[] tail=Some(259..261) strip=true",
    "full window, trailing ( after text|empty chunk|boundary=0 items=[] tail=None strip=false",
    "full window, trailing ( after text|OSC 11 query|boundary=0 items=[ColorQuery 0..7] tail=None strip=false",
    "full window, trailing ( after text|CSI 6n query|boundary=0 items=[CsiQuery 0..4] tail=None strip=false",
    "full window, trailing ( after text|text then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full window, trailing ( after text|designator pair then CSI query|boundary=0 items=[CsiQuery 3..7] tail=None strip=false",
    "full window, trailing ( after text|OSC title then CSI query|boundary=0 items=[CsiQuery 6..10] tail=None strip=false",
    "full window, trailing ( after text|OSC 777 markdown launch|boundary=0 items=[ViewerLaunch 0..34] tail=None strip=false",
    "full window, trailing ( after text|incomplete CSI|boundary=0 items=[] tail=Some(0..3) strip=true",
    "full window, trailing ( after text|incomplete OSC|boundary=0 items=[] tail=Some(0..6) strip=false",
    "full window, trailing ( after text|UTF-8 partial|boundary=0 items=[] tail=Some(0..2) strip=false",
    "full window, trailing ( after text|lone ESC|boundary=0 items=[] tail=Some(0..1) strip=false",
    "full window, trailing ( after text|ESC ESC then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full window, trailing ( after text|completing bytes of an open OSC|boundary=0 items=[] tail=None strip=false",
    "full window, trailing ( after text|ESC ( chain then OSC 11 query|boundary=0 items=[ColorQuery 4..11] tail=None strip=false",
    "full window, trailing ( after text|ESC ( chain then open CSI|boundary=0 items=[] tail=Some(4..6) strip=true",
    "full window, trailing ) after text|empty chunk|boundary=0 items=[] tail=None strip=false",
    "full window, trailing ) after text|OSC 11 query|boundary=0 items=[ColorQuery 0..7] tail=None strip=false",
    "full window, trailing ) after text|CSI 6n query|boundary=0 items=[CsiQuery 0..4] tail=None strip=false",
    "full window, trailing ) after text|text then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full window, trailing ) after text|designator pair then CSI query|boundary=0 items=[CsiQuery 3..7] tail=None strip=false",
    "full window, trailing ) after text|OSC title then CSI query|boundary=0 items=[CsiQuery 6..10] tail=None strip=false",
    "full window, trailing ) after text|OSC 777 markdown launch|boundary=0 items=[ViewerLaunch 0..34] tail=None strip=false",
    "full window, trailing ) after text|incomplete CSI|boundary=0 items=[] tail=Some(0..3) strip=true",
    "full window, trailing ) after text|incomplete OSC|boundary=0 items=[] tail=Some(0..6) strip=false",
    "full window, trailing ) after text|UTF-8 partial|boundary=0 items=[] tail=Some(0..2) strip=false",
    "full window, trailing ) after text|lone ESC|boundary=0 items=[] tail=Some(0..1) strip=false",
    "full window, trailing ) after text|ESC ESC then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full window, trailing ) after text|completing bytes of an open OSC|boundary=0 items=[] tail=None strip=false",
    "full window, trailing ) after text|ESC ( chain then OSC 11 query|boundary=0 items=[ColorQuery 4..11] tail=None strip=false",
    "full window, trailing ) after text|ESC ( chain then open CSI|boundary=0 items=[] tail=Some(4..6) strip=true",
    "full window of ( only|empty chunk|boundary=0 items=[] tail=None strip=false",
    "full window of ( only|OSC 11 query|boundary=0 items=[ColorQuery 0..7] tail=None strip=false",
    "full window of ( only|CSI 6n query|boundary=0 items=[CsiQuery 0..4] tail=None strip=false",
    "full window of ( only|text then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full window of ( only|designator pair then CSI query|boundary=0 items=[CsiQuery 3..7] tail=None strip=false",
    "full window of ( only|OSC title then CSI query|boundary=0 items=[CsiQuery 6..10] tail=None strip=false",
    "full window of ( only|OSC 777 markdown launch|boundary=0 items=[ViewerLaunch 0..34] tail=None strip=false",
    "full window of ( only|incomplete CSI|boundary=0 items=[] tail=Some(0..3) strip=true",
    "full window of ( only|incomplete OSC|boundary=0 items=[] tail=Some(0..6) strip=false",
    "full window of ( only|UTF-8 partial|boundary=0 items=[] tail=Some(0..2) strip=false",
    "full window of ( only|lone ESC|boundary=0 items=[] tail=Some(0..1) strip=false",
    "full window of ( only|ESC ESC then OSC 11 query|boundary=0 items=[ColorQuery 1..8] tail=None strip=false",
    "full window of ( only|completing bytes of an open OSC|boundary=0 items=[] tail=None strip=false",
    "full window of ( only|ESC ( chain then OSC 11 query|boundary=0 items=[ColorQuery 4..11] tail=None strip=false",
    "full window of ( only|ESC ( chain then open CSI|boundary=0 items=[] tail=Some(4..6) strip=true",
    "full window, trailing UTF-8 partial|empty chunk|boundary=2 items=[] tail=Some(0..2) strip=false",
    "full window, trailing UTF-8 partial|OSC 11 query|boundary=2 items=[ColorQuery 2..9] tail=None strip=false",
    "full window, trailing UTF-8 partial|CSI 6n query|boundary=2 items=[CsiQuery 2..6] tail=None strip=false",
    "full window, trailing UTF-8 partial|text then OSC 11 query|boundary=2 items=[ColorQuery 3..10] tail=None strip=false",
    "full window, trailing UTF-8 partial|designator pair then CSI query|boundary=2 items=[CsiQuery 5..9] tail=None strip=false",
    "full window, trailing UTF-8 partial|OSC title then CSI query|boundary=2 items=[CsiQuery 8..12] tail=None strip=false",
    "full window, trailing UTF-8 partial|OSC 777 markdown launch|boundary=2 items=[ViewerLaunch 2..36] tail=None strip=false",
    "full window, trailing UTF-8 partial|incomplete CSI|boundary=2 items=[] tail=Some(2..5) strip=true",
    "full window, trailing UTF-8 partial|incomplete OSC|boundary=2 items=[] tail=Some(2..8) strip=false",
    "full window, trailing UTF-8 partial|UTF-8 partial|boundary=2 items=[] tail=Some(2..4) strip=false",
    "full window, trailing UTF-8 partial|lone ESC|boundary=2 items=[] tail=Some(2..3) strip=false",
    "full window, trailing UTF-8 partial|ESC ESC then OSC 11 query|boundary=2 items=[ColorQuery 3..10] tail=None strip=false",
    "full window, trailing UTF-8 partial|completing bytes of an open OSC|boundary=2 items=[] tail=None strip=false",
    "full window, trailing UTF-8 partial|ESC ( chain then OSC 11 query|boundary=2 items=[ColorQuery 6..13] tail=None strip=false",
    "full window, trailing UTF-8 partial|ESC ( chain then open CSI|boundary=2 items=[] tail=Some(6..8) strip=true",
];

/// AC-3 (FR2): outside the fallback condition the scan reports, item by item
/// and tail by tail, what it reported before the change, and runs a single
/// reading.
#[test]
fn round4_as05_outside_the_fallback_the_scan_equals_the_pre_change_results() {
    let windows = corpus_windows();
    let chunks = corpus_chunks();
    assert_eq!(
        PRE_CHANGE_CORPUS.len(),
        windows.len() * chunks.len(),
        "one fixed expectation per window x chunk pair"
    );
    let mut rows = PRE_CHANGE_CORPUS.iter();
    for (window_label, window) in &windows {
        for (chunk_label, chunk) in &chunks {
            let row = rows.next().expect("a row per pair");
            let mut fields = row.splitn(3, '|');
            let (row_window, row_chunk, expected) = (
                fields.next().expect("window"),
                fields.next().expect("chunk"),
                fields.next().expect("rendering"),
            );
            assert_eq!(
                (row_window, row_chunk),
                (*window_label, *chunk_label),
                "the table follows the corpus order"
            );
            let (outcome, readings) = scan_with_reading_count(window, chunk, &[]);
            assert_eq!(readings, 1, "{window_label} / {chunk_label}: one reading");
            assert_eq!(
                render(&outcome),
                expected,
                "{window_label} / {chunk_label}: result"
            );
        }
    }
}

// ---- AC-4 (TM-2, NFR3, NFR5) ----

/// AC-4 (NFR3): the second reading runs only under the fallback condition.
/// Every shape of full window that satisfies it runs two readings; the
/// corpus windows outside it run exactly one (also asserted per row by the
/// pre-change corpus test), whatever the chunk, the empty chunk included.
#[test]
fn round4_as05_the_second_reading_runs_only_under_the_fallback_condition() {
    let mut chunks: Vec<(&str, Vec<u8>)> = corpus_chunks();
    chunks.push(("ESC ( chain only", esc_chain(b'(', 40)));
    for (window_label, window) in fallback_windows() {
        for (chunk_label, chunk) in &chunks {
            let (_outcome, readings) = scan_with_reading_count(&window, chunk, &[]);
            assert_eq!(
                readings, 2,
                "{window_label} / {chunk_label}: the fallback reads the chunk two ways"
            );
        }
    }
    for (window_label, window) in corpus_windows() {
        for (chunk_label, chunk) in &chunks {
            let (_outcome, readings) = scan_with_reading_count(&window, chunk, &[]);
            assert_eq!(
                readings, 1,
                "{window_label} / {chunk_label}: outside the fallback there is one reading"
            );
        }
    }
}

/// AC-4 (TM-2, NFR3, NFR5): long `ESC (` chains in the fallback window and
/// in the chunk, odd and even in length, with both readings, finish within
/// the test time budget and never panic.
#[test]
fn round4_as05_long_chains_in_the_window_and_the_chunk_finish_within_budget() {
    const BIG: usize = 64 * 1024;
    let mut odd_chain = esc_chain(b'(', BIG / 2);
    odd_chain.push(ESC);
    let mut paren_led_chain = vec![b'('];
    paren_led_chain.extend_from_slice(&esc_chain(b'(', BIG / 2));
    let mut long_csi = chain_then(b'(', 2, b"\x1b[");
    long_csi.resize(BIG, b'1');
    let mut long_osc = chain_then(b'(', 2, b"\x1b]");
    long_osc.resize(BIG, b'x');
    let mut long_osc_terminated = long_osc.clone();
    long_osc_terminated.push(0x07);
    let chunks: Vec<(&str, Vec<u8>)> = vec![
        ("ESC ( chain, even length", esc_chain(b'(', BIG / 2)),
        ("ESC ( chain, odd length", odd_chain),
        ("( then ESC ( chain", paren_led_chain),
        ("ESC ) chain, even length", esc_chain(b')', BIG / 2)),
        (
            "ESC ( chain then OSC 11 query",
            chain_then(b'(', BIG / 2, COLOR_QUERY),
        ),
        (
            "ESC ( chain then open CSI",
            chain_then(b'(', BIG / 2, b"\x1b["),
        ),
        ("chain then long CSI", long_csi),
        ("chain then long OSC", long_osc),
        ("chain then long terminated OSC", long_osc_terminated),
        ("all ESC", vec![ESC; BIG]),
        ("empty chunk", vec![]),
        ("lone ESC", vec![ESC]),
        ("lone (", vec![b'(']),
    ];

    let start = std::time::Instant::now();
    for (window_label, window) in fallback_windows() {
        for (chunk_label, chunk) in &chunks {
            let (outcome, readings) = scan_with_reading_count(&window, chunk, &[]);
            assert_eq!(readings, 2, "{window_label} / {chunk_label}");
            assert_eq!(
                outcome.combined.len() - outcome.boundary,
                chunk.len(),
                "{window_label} / {chunk_label}"
            );
        }
    }
    // Outside the fallback: one reading over the same chunks.
    for window in [ground_window(), vec![b'x'; 100], esc_chain(b'(', 20)] {
        for (chunk_label, chunk) in &chunks {
            let (outcome, readings) = scan_with_reading_count(&window, chunk, &[]);
            assert_eq!(readings, 1, "{chunk_label}");
            assert_eq!(
                outcome.combined.len() - outcome.boundary,
                chunk.len(),
                "{chunk_label}"
            );
        }
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "long chains must finish within the budget; took {:?}",
        start.elapsed()
    );
}

/// AC-4 (FR2): the pieces the write filter's pending run excludes keep their
/// meaning in the fallback — no tail is reported when a piece is excluded,
/// and an item overlapping a piece is dropped — while the intersection still
/// applies to the rest.
#[test]
fn round4_as05_fallback_with_excluded_pieces_reports_no_tail_and_filters_items() {
    let window = fallback_window(b'(');
    let chunk = b"x\x1b[6n\x1b[3";
    assert_eq!(
        scan(&window, chunk, &[]).tail,
        Some(5..8),
        "without a piece the open CSI is the tail"
    );
    let kept = scan(&window, chunk, &[7..8]);
    assert_eq!(
        kept.tail, None,
        "a tail is never reported with an excluded piece"
    );
    assert_eq!(kept.items.len(), 1, "the item outside the piece is kept");
    let dropped = scan(&window, chunk, &[1..5]);
    assert!(
        dropped.items.is_empty(),
        "an item overlapping a piece is dropped"
    );

    // The intersection still removes what only one reading reports.
    let chain_chunk = chain_then(b'(', 2, COLOR_QUERY);
    let outcome = scan(&window, &chain_chunk, &[0..1]);
    assert!(
        outcome.items.is_empty(),
        "the query behind the designator chain is not reported even though it lies outside the piece"
    );
    assert_eq!(outcome.tail, None);
}

// ---- TM-1 oracle ----

/// Token sequences standing in for arbitrary chunks. Each token is a short
/// piece of the constructs the scan cares about. A bare `c` is left out: after
/// an ESC it is RIS, which clears the oracle's pending responses.
const TOKENS: &[&[u8]] = &[
    b"\x1b",
    b"(",
    b"x",
    b"\x1b[c",
    b"\x1b]11;?\x07",
    b"\x1b[",
    b"\x1b]11;?",
    b"\x07",
    b"\x1b(",
];

fn token_sequences(max_len: usize) -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    let mut layer: Vec<Vec<u8>> = vec![Vec::new()];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for prefix in &layer {
            for token in TOKENS {
                let mut sequence = prefix.clone();
                sequence.extend_from_slice(token);
                next.push(sequence);
            }
        }
        all.extend(next.iter().cloned());
        layer = next;
    }
    all
}

fn responses_of(stream: &[u8]) -> Vec<u8> {
    let mut core = r2_client();
    core.process_pty_data_fully(stream);
    core.take_response()
}

/// TM-1 (FR2): for every chunk built from up to four tokens, behind a
/// fallback window whose true state is either awaiting a designator (an odd
/// chain) or ground (an even chain), the responses the reported items would
/// draw from a client never exceed the responses the raw stream draws. A
/// fabricated query would exceed them.
#[test]
fn round4_as05_intersection_never_reports_more_than_the_true_parse_answers() {
    let device_attributes = responses_of(b"\x1b[c");
    let color_response = responses_of(COLOR_QUERY);
    assert!(
        !device_attributes.is_empty(),
        "the oracle answers a DA1 query"
    );

    let window = fallback_window(b'(');
    let odd_chain = esc_chain(b'(', RETAINED_WINDOW_BYTES / 2 + 1);
    let even_chain = esc_chain(b'(', RETAINED_WINDOW_BYTES / 2);
    let mut with_items = 0usize;
    for chunk in token_sequences(4) {
        let outcome = scan(&window, &chunk, &[]);
        if outcome.items.is_empty() {
            continue;
        }
        with_items += 1;
        let item_bytes: Vec<u8> = outcome
            .items
            .iter()
            .flat_map(|item| outcome.combined[item.range.clone()].iter().copied())
            .collect();
        let from_items = responses_of(&item_bytes);
        for (parity, chain) in [("odd", &odd_chain), ("even", &even_chain)] {
            let truth = responses_of(&[chain.as_slice(), chunk.as_slice()].concat());
            for (kind, response) in [("DA1", &device_attributes), ("OSC 11", &color_response)] {
                assert!(
                    r2_count(&from_items, response) <= r2_count(&truth, response),
                    "{parity} chain, chunk {:?}: the items draw more {kind} responses than the raw stream",
                    String::from_utf8_lossy(&chunk)
                );
            }
        }
    }
    assert!(
        with_items > 0,
        "the sweep must exercise chunks whose items survive the intersection"
    );
}

// ---- AC-5 (NFR2) ----

/// AC-5 (NFR2): the scan's production code takes no lock and adds no blocking
/// wait (the scan's call sites in the suppressed pipeline are checked by diff
/// inspection).
#[test]
fn round4_as05_the_scan_takes_no_lock_and_adds_no_blocking_wait() {
    let source = include_str!("../client_parity_scan.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("the production part of the file");
    let code: String = production
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "Mutex",
        "RwLock",
        "Condvar",
        "Barrier",
        "std::sync",
        "std::thread",
        ".lock(",
        ".read(",
        ".write(",
        ".recv(",
        ".wait(",
        "sleep(",
        "park(",
        "block_on",
    ] {
        assert!(
            !code.contains(forbidden),
            "client_parity_scan.rs production code must not contain `{forbidden}`"
        );
    }
}
