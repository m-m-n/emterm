//! mux-suppressed-output-round4-fixes task0001 (FR1): the write filter holds a
//! chain of aborted strings and superseded ESCs from its head until the chain
//! settles or a cut closes it, so a later read can never complete a construct
//! the live client already closed (finding `3e2024dce619ed9f`).
//!
//! The reader-level case uses the visibility-restore harness of
//! `round3_write_path`; the rest drives the write filter and the replacement
//! builder directly. `model_chain_head` is an independent per-byte model of the
//! client's string / escape transitions, used as the oracle for what `pending`
//! must hold after every call.

use super::super::suppressed_output::{
    CarriedOverCompletion, SuppressedReplacementRequest, build_suppressed_replacement,
    build_suppressed_replacement_for,
};
use super::round3_write_path::{
    DIMS, assert_client_equals_reference, client_view, new_core, osc_introducers,
    run_reader_without_owner, run_visibility_restore_at, switch_pairs,
};
use super::*;
use std::time::{Duration, Instant};

const BUDGET: Duration = Duration::from_secs(10);

// ── the chain corpus and its oracle ──────────────────────────────────────

/// One chain of the FR1 corpus: `prefix` is settled text, `chain` runs from
/// the chain head to the end of the fed stream (the chain head is at
/// `prefix.len()`). None of the bytes is a strip target.
struct ChainForm {
    name: &'static str,
    prefix: &'static [u8],
    chain: &'static [u8],
}

const CHAIN_FORMS: &[ChainForm] = &[
    // An OSC aborted by the ESC that opens the next OSC.
    ChainForm {
        name: "OSC aborted by OSC",
        prefix: b"ab",
        chain: b"\x1b]11;?\x1b]0;x",
    },
    // An OSC aborted by a superseded ESC, then a lone ESC.
    ChainForm {
        name: "OSC then ESC ESC",
        prefix: b"ab",
        chain: b"\x1b]11;?\x1b\x1b",
    },
    // `ESC ESC` after plain text: the first ESC is superseded.
    ChainForm {
        name: "X then ESC ESC",
        prefix: b"abX",
        chain: b"\x1b\x1b",
    },
    // A DCS aborted by `ESC ]`.
    ChainForm {
        name: "DCS aborted by ESC ]",
        prefix: b"ab",
        chain: b"\x1bP0;1|x\x1b]0;t",
    },
    // An APC aborted by a lone ESC.
    ChainForm {
        name: "APC aborted by a lone ESC",
        prefix: b"ab",
        chain: b"\x1b_Xi=1;P\x1b\x1b",
    },
];

/// Independent model of the client's transitions over a whole stream starting
/// in ground: the offset of the chain head of the single incomplete construct
/// at the end of `stream`, or `None` when the stream ends in ground or right
/// after `ESC (` / `ESC )` (nothing is held then).
fn model_chain_head(stream: &[u8]) -> Option<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Ground,
        Esc,
        Designator,
        Str,
        StrEsc,
    }
    let mut state = State::Ground;
    let mut osc = false;
    let mut head: Option<usize> = None;
    for (i, &b) in stream.iter().enumerate() {
        // An ESC inside a string followed by anything but `\` aborts the
        // string: that ESC opens the next construct (the chain goes on) and
        // `b` is the byte right after it.
        let current = if state == State::StrEsc && b != b'\\' {
            State::Esc
        } else {
            state
        };
        match current {
            State::Ground => {
                if b == 0x1b {
                    state = State::Esc;
                    head = Some(i);
                }
            }
            State::Esc => match b {
                b']' => {
                    state = State::Str;
                    osc = true;
                }
                b'P' | b'_' => {
                    state = State::Str;
                    osc = false;
                }
                0x1b => state = State::Esc,
                b'(' | b')' => {
                    state = State::Designator;
                    head = None;
                }
                _ => {
                    state = State::Ground;
                    head = None;
                }
            },
            State::Designator => state = State::Ground,
            State::Str => {
                if b == 0x1b {
                    state = State::StrEsc;
                } else if b == 0x07 && osc {
                    state = State::Ground;
                    head = None;
                }
            }
            State::StrEsc => {
                // `ESC \`: the string is complete.
                state = State::Ground;
                head = None;
            }
        }
    }
    match state {
        State::Esc | State::Str | State::StrEsc => head,
        State::Ground | State::Designator => None,
    }
}

/// What `pending` must hold for `stream` fed from ground with no cut.
fn model_pending(stream: &[u8]) -> &[u8] {
    model_chain_head(stream).map_or(&[][..], |head| &stream[head..])
}

/// What a filter emitted in total and still holds after being fed in pieces.
#[derive(Debug, PartialEq)]
struct Fed {
    emitted: Vec<u8>,
    pending: Vec<u8>,
}

/// Feed `pieces` in order, asserting after EVERY call that `pending` is the
/// model's chain and that nothing from its head on has been emitted. Returns
/// the total.
fn run_pieces_checked(what: &str, pieces: &[&[u8]]) -> Fed {
    let mut filter = ScrollbackWriteFilter::new();
    let mut fed_so_far: Vec<u8> = Vec::new();
    let mut emitted: Vec<u8> = Vec::new();
    for piece in pieces {
        let (_dims, out) = filter.feed(piece, DIMS);
        emitted.extend_from_slice(&out);
        fed_so_far.extend_from_slice(piece);
        let expected_pending = model_pending(&fed_so_far);
        assert_eq!(
            filter.pending(),
            expected_pending,
            "{what}: after feeding {fed_so_far:?}, pending must be exactly the chain that \
             ends in the single incomplete construct"
        );
        assert_eq!(
            emitted,
            fed_so_far[..fed_so_far.len() - expected_pending.len()],
            "{what}: after feeding {fed_so_far:?}, exactly the bytes before the chain head \
             are emitted"
        );
    }
    Fed {
        emitted,
        pending: filter.pending().to_vec(),
    }
}

/// Every stream of the chain corpus: each form alone and followed by each
/// ending (a completion, a string terminator, a non-string escape, a
/// designation).
fn chain_streams() -> Vec<(String, Vec<u8>)> {
    let endings: [&[u8]; 9] = [
        b"",
        b"\x07",
        b"\x1b\\",
        b"\\",
        b"]11;?\x07",
        b"[31mZ",
        b"(BZ",
        b"\x1b(BZ",
        b"\x1b(",
    ];
    let mut streams = Vec::new();
    for form in CHAIN_FORMS {
        for ending in endings {
            let stream = [form.prefix, form.chain, ending].concat();
            streams.push((format!("{} + {ending:?}", form.name), stream));
        }
    }
    streams
}

// ── AC-1 (FR1, TM-1, registry) ───────────────────────────────────────────

/// AC-1 (FR1, TM-1, TS-1, registry): the three FR1 forms, each followed by a
/// removed 47 / 1047 / 1049 `h` / `l` pair, through the production visibility
/// restore of a main-screen pane whose ring has not wrapped, then a later
/// read. With the bytes before the switch in one read, and again with the
/// chain head in an earlier read. The chain the client's switch ESC closed is
/// not written to the ring, so the later read completes nothing: the ring
/// holds no OSC introducer, the client produces no response and equals the
/// raw-stream reference. The pre-fix ring holds the aborted `ESC ]11;?` (or
/// the lone ESC), which the later read completes into a color-query answer
/// the raw stream never produces.
#[test]
fn round4_3e2024dc_a_chain_closed_by_a_cut_is_never_completed_by_a_later_read() {
    struct Form {
        name: &'static str,
        // The chain head and what follows it up to the switch.
        head: &'static [u8],
        rest: &'static [u8],
        // The later read.
        later: &'static [u8],
    }
    let forms = [
        Form {
            name: "OSC aborted by OSC",
            head: b"\x1b]11;?",
            rest: b"\x1b]0;x",
            later: b"\x07",
        },
        Form {
            name: "OSC then ESC ESC",
            head: b"\x1b]11;?",
            rest: b"\x1b\x1b",
            later: b"\\",
        },
        Form {
            name: "X then ESC ESC",
            head: b"X\x1b",
            rest: b"\x1b",
            later: b"]11;?\x07",
        },
    ];
    for (enter, leave) in switch_pairs() {
        let switch = String::from_utf8_lossy(enter).into_owned();
        let pair = [enter, leave].concat();
        for form in &forms {
            // (a) everything before the switch in one read; the restore
            // covers (and suppresses) that read.
            let chunks = vec![
                [form.head, form.rest, &pair[..]].concat(),
                form.later.to_vec(),
            ];
            let ctx = format!("(a) one read, {} {switch}", form.name);
            let run = run_visibility_restore_at(&chunks, 0);
            assert_eq!(
                osc_introducers(&run.ring),
                0,
                "{ctx}: the ring holds no OSC introducer from the chain"
            );
            let (_client, responses) = client_view(&run.received);
            assert!(responses.is_empty(), "{ctx}: no response occurs");
            assert_client_equals_reference(&run.received, &chunks, &ctx);

            // (b) the chain head in an earlier read.
            let chunks = vec![
                form.head.to_vec(),
                [form.rest, &pair[..]].concat(),
                form.later.to_vec(),
            ];
            let ctx = format!("(b) head in an earlier read, {} {switch}", form.name);
            let run = run_visibility_restore_at(&chunks, 1);
            assert_eq!(
                osc_introducers(&run.ring),
                0,
                "{ctx}: the ring holds no OSC introducer from the chain"
            );
            let (_client, responses) = client_view(&run.received);
            assert!(responses.is_empty(), "{ctx}: no response occurs");
            assert_client_equals_reference(&run.received, &chunks, &ctx);
        }
    }
}

// ── AC-2 (FR1, EC-1, EC-3): a cut drops the whole chain ──────────────────

/// AC-2 (FR1, TM-1, EC-1, EC-3): a cut at any position at or after the chain
/// head emits no byte from the head on, emits the settled bytes before it, and
/// leaves nothing held — with the head in the call, with the head carried in
/// `pending` from an earlier call, and when an empty fed range with a cut at 0
/// closes the chain. Bytes after the cut start from ground, so a later ST
/// cannot complete a dropped DCS.
#[test]
fn a_cut_at_or_after_the_chain_head_drops_the_whole_chain() {
    for form in CHAIN_FORMS {
        let input = [form.prefix, form.chain].concat();
        let head = form.prefix.len();
        let settled = form.prefix.to_vec();
        let name = form.name;

        // (a) the head in the call; the fed range ends at the cut, and again
        // with plain bytes after the cut.
        for cut in head..=input.len() {
            for after in [&b""[..], &b"ZZ"[..]] {
                let fed = [&input[..cut], after].concat();
                let mut f = ScrollbackWriteFilter::new();
                let outcome = f.feed_with_cuts(&fed, DIMS, &[cut]);
                assert_eq!(
                    outcome.bytes,
                    [&settled[..], after].concat(),
                    "{name}: cut at {cut}, {after:?} after it: the settled bytes and the bytes \
                     after the cut, nothing from the chain head on"
                );
                assert!(f.pending().is_empty(), "{name}: cut at {cut}: nothing held");
                assert!(outcome.carried.is_none(), "{name}: cut at {cut}");
            }
        }

        // (b) the head carried in `pending` from an earlier call.
        for first in head + 1..=input.len() {
            let mut base = ScrollbackWriteFilter::new();
            let (_dims, out) = base.feed(&input[..first], DIMS);
            assert_eq!(
                out, settled,
                "{name}: the earlier call emits the settled bytes"
            );
            assert_eq!(
                base.pending(),
                &input[head..first],
                "{name}: the earlier call holds the chain from its head"
            );
            for cut_at in first..=input.len() {
                for after in [&b""[..], &b"ZZ"[..]] {
                    let mut f = ScrollbackWriteFilter::new();
                    let (_dims, mut emitted) = f.feed(&input[..first], DIMS);
                    let fed = [&input[first..cut_at], after].concat();
                    let outcome = f.feed_with_cuts(&fed, DIMS, &[cut_at - first]);
                    emitted.extend_from_slice(&outcome.bytes);
                    assert_eq!(
                        emitted,
                        [&settled[..], after].concat(),
                        "{name}: head carried (first call {first}), cut at {cut_at}"
                    );
                    assert!(f.pending().is_empty(), "{name}: carried, cut at {cut_at}");
                    assert!(
                        outcome.carried.is_none(),
                        "{name}: carried, cut at {cut_at}"
                    );
                }
            }

            // (c) an empty fed range with a cut at 0 closes the carried chain.
            let mut f = ScrollbackWriteFilter::new();
            f.feed(&input[..first], DIMS);
            let outcome = f.feed_with_cuts(b"", DIMS, &[0]);
            assert!(
                outcome.bytes.is_empty(),
                "{name}: the closing writes nothing from the chain"
            );
            assert!(
                f.pending().is_empty(),
                "{name}: the closing leaves nothing held"
            );
            assert!(outcome.carried.is_none());

            // A later ST cannot complete a dropped string: nothing the chain
            // opened is in what the filter emitted.
            let mut total = settled.clone();
            let (_dims, later) = f.feed(b"\x1b\\Z", DIMS);
            total.extend_from_slice(&later);
            for intro in [&b"\x1b]"[..], &b"\x1bP"[..], &b"\x1b_"[..]] {
                assert_eq!(
                    r2_count(&total, intro),
                    0,
                    "{name}: no {intro:?} introducer is ever emitted from the chain"
                );
            }
        }
    }

    // Two chains with a settled construct between them: only the chain the
    // cut closes is dropped.
    let mut f = ScrollbackWriteFilter::new();
    let fed = b"ab\x1b]1;a\x1b]2;b\x07cd\x1b]3;c\x1b\x1b";
    let outcome = f.feed_with_cuts(fed, DIMS, &[fed.len()]);
    assert_eq!(
        outcome.bytes,
        b"ab\x1b]1;a\x1b]2;b\x07cd".to_vec(),
        "the first chain completed (settled and written); the second is closed by the cut"
    );
    assert!(f.pending().is_empty());
}

/// AC-2 (FR1, EC-1): the reader's fallback closing (a switch that straddles
/// reads, with the alternate screen involved) feeds an empty range with one cut
/// at fed 0, which drops a chain held from earlier reads from its head. Only
/// the settled text and the bytes after the switch reach the ring. The shadow
/// parser does not enter the alternate screen on `?1047`, so a straddling
/// `?1047h` never reaches the fallback; only the 47 and 1049 forms do.
#[test]
fn the_reader_fallback_drops_a_held_chain_from_its_head() {
    for (enter, leave) in switch_pairs() {
        if enter == b"\x1b[?1047h" {
            continue;
        }
        let switch = String::from_utf8_lossy(enter).into_owned();
        // The straddle splits each sequence after its ESC.
        let chunks = vec![
            b"x\x1b]11;?\x1b]0;t".to_vec(),
            b"\x1b".to_vec(),
            enter[1..].to_vec(),
            b"\x1b".to_vec(),
            leave[1..].to_vec(),
            b"\x07z".to_vec(),
        ];
        let ring = run_reader_without_owner(chunks);
        assert_eq!(
            osc_introducers(&ring),
            0,
            "{switch}: the held chain is not written to the ring"
        );
        assert_eq!(
            ring,
            b"x\x07z".to_vec(),
            "{switch}: the chain is dropped from its head, nothing completes it"
        );
    }
}

// ── AC-3 (FR1, SPEC AC-4): split invariance over the chain corpus ────────

/// AC-3 (FR1, TS-2, SPEC AC-4): for every split position and for
/// byte-at-a-time feeding of the chain corpus, with and without a final
/// completion, the total emitted bytes and `pending` equal the single-call
/// result. After every call `pending` is empty or exactly the chain that ends
/// in the single incomplete construct, so the chain head is never emitted by
/// the read that opened it.
#[test]
fn chain_corpus_is_split_invariant_and_pending_is_the_whole_chain_after_every_call() {
    for (what, stream) in chain_streams() {
        let whole = run_pieces_checked(&what, &[&stream]);
        for split in 0..=stream.len() {
            let got = run_pieces_checked(&what, &[&stream[..split], &stream[split..]]);
            assert_eq!(
                got, whole,
                "{what}: split at {split} of {stream:?} differs from a single call"
            );
        }
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(
            run_pieces_checked(&what, &bytes),
            whole,
            "{what}: byte-at-a-time feeding of {stream:?} differs from a single call"
        );
    }
}

/// AC-3: the chain head is not written by the read that opened it. A read that
/// ends inside a chain leaves everything from the head on in `pending`, for
/// each form.
#[test]
fn the_read_that_opens_a_chain_does_not_write_its_head() {
    for form in CHAIN_FORMS {
        let input = [form.prefix, form.chain].concat();
        let mut f = ScrollbackWriteFilter::new();
        let (_dims, out) = f.feed(&input, DIMS);
        assert_eq!(
            out, form.prefix,
            "{}: only the settled prefix is written",
            form.name
        );
        assert_eq!(
            f.pending(),
            form.chain,
            "{}: the chain is held whole",
            form.name
        );
        // The construct start is the opening ESC of the last construct; the
        // scan of the held run resumes there.
        assert_eq!(
            f.held_construct_start(),
            form.chain.iter().rposition(|&b| b == 0x1b),
            "{}: the construct start is stored with the chain",
            form.name
        );
    }
}

// ── AC-4 (FR1, EC-2, TS-3): the carried-over completion ──────────────────

/// AC-4 (FR1, EC-2, TS-3): when the held construct completes in a later call
/// before any cut, the carried-over completion carries exactly that
/// construct's bytes (from its opening ESC through its terminator, never the
/// chain head) and its fed end, and the whole held run is emitted.
#[test]
fn a_held_construct_that_completes_reports_only_its_own_bytes_and_settles_the_chain() {
    // (held chain, the later read, the construct that completes, its fed end)
    let cases: [(&[u8], &[u8], &[u8], usize); 5] = [
        (b"ab\x1b]11;?\x1b]11;?", b"\x07", b"\x1b]11;?\x07", 1),
        (b"ab\x1b]11;?\x1b]0;x", b"y\x1b\\", b"\x1b]0;xy\x1b\\", 3),
        (b"abX\x1b\x1b", b"]11;?\x07", b"\x1b]11;?\x07", 6),
        (b"ab\x1bP0;1|x\x1b]0;t", b"\x07", b"\x1b]0;t\x07", 1),
        (b"ab\x1b_Xi=1;P\x1b\x1b", b"_Xj\x1b\\", b"\x1b_Xj\x1b\\", 5),
    ];
    for (held, later, construct, fed_end) in cases {
        let head = held.iter().position(|&b| b == 0x1b).unwrap();
        let mut f = ScrollbackWriteFilter::new();
        let (_dims, first) = f.feed(held, DIMS);
        assert_eq!(first, &held[..head]);
        let chain = f.pending().to_vec();
        assert_eq!(chain, &held[head..], "{held:?}: held from the chain head");

        let outcome = f.feed_with_cuts(later, DIMS, &[]);
        assert_eq!(
            outcome.bytes,
            [&chain[..], later].concat(),
            "{held:?}: the whole held run is emitted"
        );
        assert!(f.pending().is_empty());
        let carried = outcome
            .carried
            .unwrap_or_else(|| panic!("{held:?}: the completion is reported"));
        assert_eq!(
            carried.bytes(),
            construct,
            "{held:?}: exactly the completed construct, never an aborted link"
        );
        assert_eq!(carried.fed_end(), fed_end, "{held:?}: fed end");
    }

    // A held construct that is itself ABORTED is never reported, even when a
    // later construct in the same call completes.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b]11;?", DIMS);
    let outcome = f.feed_with_cuts(b"\x1b]0;x\x07", DIMS, &[]);
    assert_eq!(outcome.bytes, b"\x1b]11;?\x1b]0;x\x07".to_vec());
    assert!(
        outcome.carried.is_none(),
        "an aborted chain link is never reported"
    );

    // A chain still open after the call reports nothing and stays held whole.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"ab\x1b]11;?\x1b]0;x", DIMS);
    let outcome = f.feed_with_cuts(b"yy\x1b]1;", DIMS, &[]);
    assert!(outcome.bytes.is_empty());
    assert!(outcome.carried.is_none());
    assert_eq!(f.pending(), b"\x1b]11;?\x1b]0;xyy\x1b]1;".as_slice());
}

/// AC-4 (EC-2): a chain that settles before a cut is written; the construct
/// opened after it is dropped by the cut. The completion is reported for the
/// construct start.
#[test]
fn a_chain_that_settles_before_a_cut_is_written_and_the_next_construct_is_dropped() {
    let mut f = ScrollbackWriteFilter::new();
    let (_dims, first) = f.feed(b"ab\x1b]11;?\x1b]0;t", DIMS);
    assert_eq!(first, b"ab");

    let fed = b"\x07cd\x1b]1;u\x1b\x1b";
    let outcome = f.feed_with_cuts(fed, DIMS, &[fed.len()]);
    assert_eq!(
        outcome.bytes,
        b"\x1b]11;?\x1b]0;t\x07cd".to_vec(),
        "the settled chain is written; the chain opened after it is dropped by the cut"
    );
    let carried = outcome
        .carried
        .expect("the completion before the cut is reported");
    assert_eq!(carried.bytes(), b"\x1b]0;t\x07");
    assert_eq!(carried.fed_end(), 1);
    assert!(f.pending().is_empty());

    // The completion ends exactly at the cut.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b]11;?\x1b]0;t", DIMS);
    let outcome = f.feed_with_cuts(b"\x07", DIMS, &[1]);
    assert_eq!(outcome.bytes, b"\x1b]11;?\x1b]0;t\x07".to_vec());
    let carried = outcome.carried.expect("reported");
    assert_eq!(carried.bytes(), b"\x1b]0;t\x07");
    assert_eq!(carried.fed_end(), 1);
}

fn responses_of(bytes: &[u8]) -> Vec<u8> {
    let mut core = new_core();
    core.process_pty_data_fully(bytes);
    core.take_response()
}

/// AC-4 (TS-3): for a suppressed chunk whose `pending` holds a chain, the
/// re-delivered tail is the whole chain, and no byte is both a replacement item
/// and part of the tail: a complete query before the chain is delivered once,
/// the chain is delivered once, and the client answers what the raw stream
/// answers.
#[test]
fn a_suppressed_chunk_whose_pending_holds_a_chain_redelivers_it_once() {
    let query = b"\x1b]11;?\x07";
    let chain = b"\x1b]11;?\x1b]0;x";
    let chunk = [&b"a"[..], query, b"b", chain].concat();
    let mut f = ScrollbackWriteFilter::new();
    let outcome = f.feed_with_cuts(&chunk, DIMS, &[]);
    assert_eq!(f.pending(), chain.as_slice(), "the chain is held whole");
    assert!(outcome.carried.is_none());
    let replacement = build_suppressed_replacement(&chunk, &[0..chunk.len()], f.pending(), &[]);
    assert_eq!(
        replacement,
        [&query[..], &chain[..]].concat(),
        "the query once, then the whole chain as the tail"
    );
    // Parse parity: the replacement and a later BEL answer what the raw
    // stream answers.
    let later = b"\x07";
    assert_eq!(
        responses_of(&[&replacement[..], later].concat()),
        responses_of(&[&chunk[..], later].concat()),
        "client responses equal the raw-stream reference"
    );

    // A chain head from an earlier read: the whole chain is the tail, and
    // every byte of this chunk belongs to it.
    let read0 = b"\x1b]11;?";
    let read1 = b"\x1b]11;?\x1b]0;x";
    let mut f = ScrollbackWriteFilter::new();
    f.feed(read0, DIMS);
    f.feed(read1, DIMS);
    let held = [&read0[..], &read1[..]].concat();
    assert_eq!(f.pending(), held.as_slice());
    let replacement = build_suppressed_replacement(read1, &[0..read1.len()], f.pending(), read0);
    assert_eq!(
        replacement, held,
        "the whole held chain is re-delivered, once, with no item from it"
    );
    assert_eq!(
        responses_of(&[&replacement[..], later].concat()),
        responses_of(&[&held[..], later].concat()),
    );

    // A cut before the chain, with queries on both sides of the removed
    // switch: the queries are delivered once each, in order; the chain after
    // the last cut is the tail.
    let chunk = [
        &b"\x1b]11;?\x07"[..],
        b"\x1b[?1049h",
        b"\x1b]11;?\x07",
        b"\x1b[?1049l",
        b"X\x1b]11;?\x1b\x1b",
    ]
    .concat();
    let (main, _alt, spans) = extract_main_buffer_bytes(&chunk, false);
    let cuts = cuts_from_main_spans(&spans, chunk.len());
    let mut f = ScrollbackWriteFilter::new();
    let outcome = f.feed_with_cuts(&main, DIMS, &cuts);
    assert_eq!(f.pending(), b"\x1b]11;?\x1b\x1b".as_slice());
    assert_eq!(outcome.bytes, [&b"\x1b]11;?\x07"[..], b"X"].concat());
    let replacement = build_suppressed_replacement(&chunk, &spans, f.pending(), &[]);
    assert_eq!(
        replacement,
        [
            &b"\x1b]11;?\x07"[..],
            b"\x1b]11;?\x07",
            b"\x1b]11;?\x1b\x1b"
        ]
        .concat(),
        "both queries once, in stream order, then the chain"
    );
    assert_eq!(
        responses_of(&[&replacement[..], b"\\"].concat()),
        responses_of(&[&chunk[..], b"\\"].concat()),
    );

    // A carried-over completion of a held chain: the construct is delivered
    // once (the scan item and the carried report are the same sequence).
    let read0 = b"\x1b]11;?\x1b]11;?";
    let mut f = ScrollbackWriteFilter::new();
    f.feed(read0, DIMS);
    let read1 = b"\x07";
    let outcome = f.feed_with_cuts(read1, DIMS, &[]);
    let carried = outcome.carried.expect("the held construct completed");
    let replacement = build_suppressed_replacement_for(&SuppressedReplacementRequest {
        chunk: read1,
        ring_written_ranges: &[0..1],
        pending_after: f.pending(),
        window: read0,
        snapshot_trailing_construct: None,
        carried_over_completion: Some(CarriedOverCompletion {
            bytes: carried.bytes(),
            end: carried.fed_end(),
        }),
    });
    assert_eq!(
        replacement, b"\x1b]11;?\x07",
        "the completed query is delivered once, never joined with an aborted link"
    );
}

// ── AC-5 (FR1, NFR5, TM-2, EC-6, TS-4): the cap and hostile chains ───────

/// `ESC ]` pairs: each aborts the previous OSC and opens a new one.
fn introducer_pairs(count: usize) -> Vec<u8> {
    std::iter::repeat_n(*b"\x1b]", count).flatten().collect()
}

/// AC-5 (FR1, NFR5, EC-6): a run of aborted strings that grows the held chain
/// past the 512 KiB cap takes the strip-filtered overflow flush and does not
/// panic. Afterwards `pending` is empty and the awaiting-designator flag
/// equals the client-parity state at the end of the flushed run, whether the
/// chain was held across calls or fed in one.
#[test]
fn a_held_chain_past_the_cap_takes_the_overflow_flush() {
    let cap_pairs = SCROLLBACK_FILTER_PENDING_CAP / 2;

    // One large feed crosses the cap.
    for (tail, awaiting) in [
        (&b""[..], false),
        (&b"\x1b("[..], true),
        (&b"\x1b)"[..], true),
        (&b"\x1b(\x1b"[..], false),
        (&b"\x1b]0;x"[..], false),
    ] {
        let mut run = introducer_pairs(cap_pairs + 8);
        run.extend_from_slice(tail);
        let mut f = ScrollbackWriteFilter::new();
        let start = Instant::now();
        let outcome = f.feed_with_cuts(&run, DIMS, &[]);
        assert!(
            start.elapsed() < BUDGET,
            "tail {tail:?}: took {:?}",
            start.elapsed()
        );
        assert_eq!(
            outcome.bytes, run,
            "tail {tail:?}: the flush emits the stripped run"
        );
        assert!(outcome.carried.is_none());
        assert!(
            f.pending().is_empty(),
            "tail {tail:?}: nothing held after the flush"
        );
        assert_eq!(
            f.held_construct_start(),
            None,
            "tail {tail:?}: no construct start is stored after the flush"
        );
        assert_eq!(
            f.awaiting_designator(),
            awaiting,
            "tail {tail:?}: the flag follows the end of the flushed run"
        );
    }

    // The chain is held across calls (each within a read buffer), then
    // crosses the cap on the ninth read (9 x 60,000 bytes > 512 KiB).
    let piece = introducer_pairs(30_000);
    let crossing = SCROLLBACK_FILTER_PENDING_CAP / piece.len();
    for (tail, awaiting) in [(&b""[..], false), (&b"\x1b("[..], true)] {
        let mut f = ScrollbackWriteFilter::new();
        let mut held_len = 0usize;
        for read in 0..=crossing {
            let mut fed = piece.clone();
            if read == crossing {
                fed.extend_from_slice(tail);
            }
            let outcome = f.feed_with_cuts(&fed, DIMS, &[]);
            if read < crossing {
                assert!(
                    outcome.bytes.is_empty(),
                    "read {read}: the chain is held whole"
                );
                held_len += fed.len();
                assert_eq!(f.pending().len(), held_len, "read {read}");
            } else {
                assert_eq!(
                    outcome.bytes.len(),
                    held_len + fed.len(),
                    "the flush emits the held chain with the read that crossed the cap"
                );
                assert!(
                    f.pending().is_empty(),
                    "tail {tail:?}: nothing held after the flush"
                );
                assert!(outcome.carried.is_none());
            }
        }
        assert_eq!(f.awaiting_designator(), awaiting, "tail {tail:?}");
        assert_eq!(
            f.held_construct_start(),
            None,
            "tail {tail:?}: no construct start is stored after the flush"
        );
    }

    // After the flush the filter behaves as a fresh one.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(&introducer_pairs(cap_pairs + 8), DIMS);
    assert!(f.pending().is_empty());
    let mut fresh = ScrollbackWriteFilter::new();
    for piece in [&b"ab\x1b]0;"[..], b"x\x07cd\x1b\x1b", b"]11;?\x07"] {
        let got = f.feed_with_cuts(piece, DIMS, &[]);
        let want = fresh.feed_with_cuts(piece, DIMS, &[]);
        assert_eq!(got.bytes, want.bytes);
        assert_eq!(f.pending(), fresh.pending());
    }
}

/// AC-5 (TS-4, TS-10): a 64 KiB hostile aborted-introducer stream, fed whole,
/// split and byte at a time, is one chain below the cap, finishes within the
/// budget and settles whole when its last construct completes.
#[test]
fn a_hostile_aborted_introducer_stream_is_one_linear_chain() {
    let chain = introducer_pairs(32 * 1024); // 64 KiB

    let start = Instant::now();
    let mut f = ScrollbackWriteFilter::new();
    let (_dims, out) = f.feed(&chain, DIMS);
    assert!(out.is_empty(), "the whole chain is held");
    assert_eq!(f.pending(), chain.as_slice());
    assert_eq!(
        f.held_construct_start(),
        Some(chain.len() - 2),
        "the scan of the held run resumes at the final pair, not at the chain head"
    );
    let outcome = f.feed_with_cuts(b"\x07", DIMS, &[]);
    assert_eq!(outcome.bytes, [&chain[..], b"\x07"].concat());
    assert_eq!(outcome.carried.expect("completed").bytes(), b"\x1b]\x07");
    assert!(f.pending().is_empty());

    for split in [1usize, 2, 3, 4, 5, chain.len() / 2, chain.len() - 1] {
        let mut f = ScrollbackWriteFilter::new();
        let (_dims, mut emitted) = f.feed(&chain[..split], DIMS);
        let (_dims, out) = f.feed(&chain[split..], DIMS);
        emitted.extend_from_slice(&out);
        assert!(emitted.is_empty(), "split at {split}: nothing is written");
        assert_eq!(f.pending(), chain.as_slice(), "split at {split}");
    }

    let mut f = ScrollbackWriteFilter::new();
    for byte in chain.chunks(1) {
        let (_dims, out) = f.feed(byte, DIMS);
        assert!(out.is_empty());
    }
    assert_eq!(f.pending(), chain.as_slice(), "byte at a time");
    assert!(
        start.elapsed() < BUDGET,
        "hostile chains must scan in linear time; took {:?}",
        start.elapsed()
    );

    // A cut closes the whole chain at once.
    let outcome = f.feed_with_cuts(b"", DIMS, &[0]);
    assert!(outcome.bytes.is_empty());
    assert!(f.pending().is_empty());
}
