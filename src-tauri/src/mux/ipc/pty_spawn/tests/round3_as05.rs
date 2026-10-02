//! mux-suppressed-output-round3-fixes task0003 (FR5): reader-level tests of
//! the as-05 fallback with an undecidable trailing designator introducer.
//!
//! The reader keeps the last [`RETAINED_WINDOW_BYTES`] bytes of the stream as
//! the retention window. When that window is full, holds no ESC the
//! designator-slot exclusion accepts as a restart position, and ends in
//! `ESC (` / `ESC )`, the chunk's first byte may already have been consumed
//! by the client as a designator, so the replacement of a suppressed chunk
//! must not extract a query or launch from a construct starting there. The
//! rule only ever causes misses.
//!
//! Oracle convention (IMPLEMENTATION.md "Reader-level oracle convention"):
//! responses are always compared with the raw-stream reference; the screen is
//! compared when the stand-in snapshot itself reproduces the reference's
//! screen for its prefix (`r2_snapshot_reproduces_prefix`).

use super::*;
use crate::mux::ipc::pty_spawn::client_parity_scan::RETAINED_WINDOW_BYTES;

const ESC: u8 = 0x1b;

/// The suppressed chunk: an OSC 11 color query.
const COLOR_QUERY: &[u8] = b"\x1b]11;?\x07";

/// `pairs` consecutive `ESC (`.
fn esc_paren_chain(pairs: usize) -> Vec<u8> {
    [ESC, b'('].repeat(pairs)
}

/// Run `chunks` through the production reader with read `1` (the last one)
/// covered by a stand-in snapshot, and compare the client with the raw-stream
/// reference.
///
/// Returns the run and whether the screen comparison was made (it is made
/// when the stand-in snapshot reproduces the reference's screen for its own
/// prefix, the oracle convention).
fn run_and_compare(chunks: &[Vec<u8>], exact_responses: bool, ctx: &str) -> (SuppressedRun, bool) {
    assert_eq!(
        chunks.len(),
        2,
        "{ctx}: earlier reads, then the suppressed read"
    );
    let run = run_reader_with_suppressed_reads(chunks, &[1]);
    assert_r2_responses_match(&run, chunks, exact_responses, ctx);
    let stream = chunks.concat();
    let compared = r2_snapshot_reproduces_prefix(&run, &stream, stream.len());
    if compared {
        assert_r2_screen_matches(&run, chunks, ctx);
    }
    (run, compared)
}

/// AC-3 (FR5, TM-1, TS-5; registry `eaf83fe08869d5e6`): 129 consecutive
/// `ESC (` in earlier reads leave the retention window full, with no
/// decidable ESC and ending in `ESC (`. The suppressed chunk
/// `ESC ]11;? BEL` then starts with the byte the client consumed as the
/// designator (an odd chain: the last `ESC (` is a real introducer), so the
/// client's parser never saw an OSC. No replacement chunk is sent for the
/// chunk and the client produces no response — the raw-stream reference
/// produces none either.
#[test]
fn round3_eaf83fe0_as05_fallback_never_fabricates_after_a_trailing_esc_paren() {
    let chain = esc_paren_chain(RETAINED_WINDOW_BYTES / 2 + 1);
    assert_eq!(chain.len(), RETAINED_WINDOW_BYTES + 2, "129 pairs");
    let chunks = vec![chain.clone(), COLOR_QUERY.to_vec()];

    let (run, _screen_compared) =
        run_and_compare(&chunks, true, "129 ESC ( then a suppressed query");

    assert_eq!(
        run.pty_output(),
        vec![chain],
        "only the earlier read reaches the client as output; nothing is sent for the suppressed chunk"
    );
    assert_eq!(
        r2_count(&run.pty_output_bytes(), COLOR_QUERY),
        0,
        "the query must not be fabricated"
    );
    let (_client, client_responses) = r2_client_view(&run);
    assert!(
        client_responses.is_empty(),
        "the client produces no response: {client_responses:?}"
    );
    let (_reference, reference_responses) = r2_reference(&chunks);
    assert!(
        reference_responses.is_empty(),
        "the raw stream's reference produces none either: {reference_responses:?}"
    );
}

/// AC-3 (FR5, accepted miss): the same stream with 128 pairs. The chain is
/// even, so the reference does answer the query, but the window alone cannot
/// tell the two parities apart; the rule misses it. Only fabrication is
/// asserted: whatever the client answers equals what the reference answered.
#[test]
fn round3_as05_even_chain_never_answers_more_than_the_reference() {
    let chunks = vec![
        esc_paren_chain(RETAINED_WINDOW_BYTES / 2),
        COLOR_QUERY.to_vec(),
    ];
    let run = run_reader_with_suppressed_reads(&chunks, &[1]);
    let (_client, client_responses) = r2_client_view(&run);
    let (_reference, reference_responses) = r2_reference(&chunks);
    assert!(
        client_responses.is_empty() || client_responses == reference_responses,
        "no response may be fabricated: client {client_responses:?}, reference {reference_responses:?}"
    );
    assert!(
        r2_count(&run.pty_output_bytes(), COLOR_QUERY) <= 1,
        "the query is delivered at most once"
    );
}

/// AC-2 (FR5, unaffected case), through the reader: the retention window is
/// full of `(` only, so its trailing `(` is not preceded by ESC and the scan
/// starts at the chunk. The suppressed query is delivered once and the client
/// answers exactly as the reference does.
#[test]
fn round3_as05_trailing_paren_not_preceded_by_esc_still_delivers_the_query() {
    let chunks = vec![vec![b'('; RETAINED_WINDOW_BYTES + 44], COLOR_QUERY.to_vec()];
    let (run, _screen_compared) = run_and_compare(&chunks, true, "( bytes then a suppressed query");

    assert_eq!(
        r2_count(&run.pty_output_bytes(), COLOR_QUERY),
        1,
        "the query is still re-delivered once"
    );
    let (_reference, reference_responses) = r2_reference(&chunks);
    let (_client, client_responses) = r2_client_view(&run);
    assert_eq!(client_responses, reference_responses);
}
