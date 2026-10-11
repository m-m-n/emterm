//! mux-osc-abort-altscreen-split-read task0001 (FR1-FR8, NFR1-NFR5): the
//! reader carries the partial state of an alternate-screen switch sequence from
//! one read to the next ([`AltSpanTracker`]) and hands the agent-status feed
//! scanner, for every read, the live spans the bulk extraction of the same bytes
//! gives, so a mux pane ends in the plain tab's OSC 7501 table and OSC 777 state
//! however the switch sequences are split. The scrollback write path is
//! unchanged.
//!
//! Test identifiers (task plan): AC-1 the span equality over the tracker corpus,
//! AC-2 the scanner items, AC-4 the write-path pin, AC-5 the bound and the
//! no-panic stress, AC-6 the unmodelled forms, the parser rebuild and the
//! shadow-parser pin. AC-3 lives in `daemon/tests.rs` (the real reader loop and
//! the daemon task).
//!
//! Oracle convention: the reference is the main-buffer extraction run once over
//! the whole stream, restricted to each read. A read's undecided tail (the bytes
//! of a live prefix of a switch sequence the read ends in) is not live in the
//! tracker's result whatever the bulk says: the scanner decides nothing on those
//! bytes (AC-2 shows the items do not change).

use super::*;
use crate::mux::session::pane::{ShadowParser, new_shadow_parser};
use std::ops::Range;

const ESC: u8 = 0x1b;
/// `ESC ] 133;A` with no terminator.
const A: &[u8] = b"\x1b]133;A";
const BEL: &[u8] = b"\x07";
const ST: &[u8] = b"\x1b\\";
/// The second `ESC` cuts the string; the `\` after it ends nothing.
const EE: &[u8] = b"\x1b\x1b\\";
/// The `[` cuts the string and starts a harmless SGR reset.
const EX: &[u8] = b"\x1b[0m";
/// `ESC (`: the byte after it is a charset designator.
const DS: &[u8] = b"\x1b(";
/// A prefix of ?1047 / ?1049 followed by a byte that completes no switch.
const X: &[u8] = b"\x1b[?104x";

/// The switch patterns, restated (the extraction's list is private to
/// `write_filter`): `(pattern, enters the alternate screen)`.
const PATTERNS: [(&[u8], bool); 6] = [
    (b"\x1b[?1049h", true),
    (b"\x1b[?1049l", false),
    (b"\x1b[?1047h", true),
    (b"\x1b[?1047l", false),
    (b"\x1b[?47h", true),
    (b"\x1b[?47l", false),
];

/// The cut forms of the SC-1 corpus, restated (the list is private to
/// `program_status_feed`): placed right after the bytes of an OSC string.
const CUT_FORMS: [(&str, &[u8]); 4] = [
    ("ESC ESC \\", b"\x1b\x1b\\"),
    ("ESC [0m", b"\x1b[0m"),
    ("ESC ] 0;t BEL", b"\x1b]0;t\x07"),
    ("ESC c", b"\x1bc"),
];

fn enter(p: u16) -> Vec<u8> {
    format!("\x1b[?{p}h").into_bytes()
}

fn leave(p: u16) -> Vec<u8> {
    format!("\x1b[?{p}l").into_bytes()
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn prompt_start() -> AgentStatusFeedItem {
    AgentStatusFeedItem::Osc133Mark(crate::prompts::PromptMarkKind::PromptStart)
}

// ---- the corpus ----

/// The streams of SC-1x rows K16 to K37 (IMPLEMENTATION.md), built from the
/// notation there.
fn sc1x_streams() -> Vec<(String, Vec<u8>)> {
    let mut rows: Vec<(String, Vec<u8>)> = Vec::new();
    let mut row = |name: String, stream: Vec<u8>| rows.push((name, stream));
    let (e49, l49) = (enter(1049), leave(1049));
    let (e1047, l1047) = (enter(1047), leave(1047));
    let (e47, l47) = (enter(47), leave(47));

    row("K16".into(), cat(&[&e49, &l49, A, EE]));
    let mut n = 17;
    for (e, l) in [(&e49, &l49), (&e1047, &l1047)] {
        for tail in [BEL, ST, EE, EX] {
            row(format!("K{n}"), cat(&[e, l, A, tail]));
            n += 1;
        }
    }
    // K17 to K24 are the 1049 and 1047 rows; K25 to K28 are the 47 rows.
    for tail in [BEL, ST, EE, EX] {
        row(format!("K{n}"), cat(&[&e47, &l47, A, tail]));
        n += 1;
    }
    row("K29".into(), cat(&[&e49, &l49, A, BEL, &e49]));
    row("K30".into(), cat(&[&e1047, &l1047, A, BEL, &e1047]));
    row("K31".into(), cat(&[&e47, &l47, A, BEL, &e47]));
    row("K32".into(), cat(&[&e49, A, BEL, &l49]));
    row("K33".into(), cat(&[&e1047, A, BEL, &l1047]));
    row("K34".into(), cat(&[&e47, A, BEL, &l47]));
    row("K35".into(), cat(&[DS, &e49, A, BEL]));
    row("K36".into(), cat(&[&e49, DS, &l49, A, BEL]));
    row("K37".into(), cat(&[X, A, BEL]));
    rows
}

/// Every stream of the tracker corpus (task plan, Test Notes).
fn tracker_corpus() -> Vec<(String, Vec<u8>)> {
    let mut corpus = sc1x_streams();
    let (e49, l49) = (enter(1049), leave(1049));

    // For each P: E(P) L(P) A followed by each cut form.
    for p in [1049u16, 1047, 47] {
        for (form, bytes) in CUT_FORMS {
            corpus.push((
                format!("E({p}) L({p}) A + {form}"),
                cat(&[&enter(p), &leave(p), A, bytes]),
            ));
        }
    }
    // K35 and K36 with `ESC )` in place of `ESC (`.
    let ds_close: &[u8] = b"\x1b)";
    corpus.push(("K35 with ESC )".into(), cat(&[ds_close, &e49, A, BEL])));
    corpus.push((
        "K36 with ESC )".into(),
        cat(&[&e49, ds_close, &l49, A, BEL]),
    ));
    // Prefix ambiguity: E(1049) and E(1047), each followed by `ESC [?104`
    // `9l` and by `ESC [?104` `7l`.
    for e in [1049u16, 1047] {
        for l in [1049u16, 1047] {
            corpus.push((
                format!("E({e}) then ESC[?{l}l"),
                cat(&[&enter(e), &leave(l), A, BEL]),
            ));
        }
    }
    // `ESC [?10` `4x` on the main and on the alternate screen.
    corpus.push(("ESC[?104x on main".into(), cat(&[X, A, BEL])));
    corpus.push(("ESC[?104x on alt".into(), cat(&[&e49, X, A, BEL])));
    // K17's stream followed by an unfinished `ESC [?10`: an undecided tail
    // that never completes.
    corpus.push((
        "K17 + unfinished ESC[?10".into(),
        cat(&[&e49, &l49, A, BEL, b"\x1b[?10"]),
    ));
    corpus
}

/// Every delivery of a stream of `len` bytes: the whole stream in one read,
/// every single cut, every pair of cuts, and one byte per read. A delivery is
/// the ascending cut offsets (empty: one read).
fn deliveries(len: usize) -> Vec<Vec<usize>> {
    let mut all = vec![Vec::new()];
    for o in 1..len {
        all.push(vec![o]);
    }
    for a in 1..len {
        for b in a + 1..len {
            all.push(vec![a, b]);
        }
    }
    all.push((1..len).collect());
    all
}

// ---- the rig: a tracker beside a shadow parser built as the reader builds it ----

struct Rig {
    tracker: AltSpanTracker,
    parser: ShadowParser,
}

impl Rig {
    fn new() -> Self {
        Self {
            tracker: AltSpanTracker::new(),
            parser: new_shadow_parser(24, 80),
        }
    }

    /// One read: the parser processes it, the tracker is advanced with the
    /// parser's states before and after it.
    fn read(&mut self, bytes: &[u8]) -> Vec<Range<usize>> {
        let before = self.parser.screen().alternate_screen();
        self.parser.process(bytes);
        let after = self.parser.screen().alternate_screen();
        self.tracker.advance(bytes, before, after)
    }
}

struct ReadRun {
    lo: usize,
    hi: usize,
    spans: Vec<Range<usize>>,
    partial_len: usize,
}

/// `stream` cut at the ascending `offsets`, one read per piece, through a
/// [`Rig`]. Returns every read's result and the number of times a check
/// disagreed with the shadow parser.
fn run_tracker(stream: &[u8], offsets: &[usize]) -> (Vec<ReadRun>, u32) {
    let mut rig = Rig::new();
    let mut runs = Vec::new();
    let mut lo = 0;
    for &hi in offsets.iter().chain(std::iter::once(&stream.len())) {
        let spans = rig.read(&stream[lo..hi]);
        runs.push(ReadRun {
            lo,
            hi,
            spans,
            partial_len: rig.tracker.partial_len(),
        });
        lo = hi;
    }
    (runs, rig.tracker.adoptions())
}

// ---- the oracle ----

/// The part of `spans` (ranges of a whole stream) that falls inside `lo..hi`,
/// rebased to start at `lo`. Restated: the original is private to
/// `program_status_feed`.
fn spans_within(spans: &[Range<usize>], lo: usize, hi: usize) -> Vec<Range<usize>> {
    spans
        .iter()
        .filter_map(|r| {
            let (start, end) = (r.start.max(lo), r.end.min(hi));
            (start < end).then(|| start - lo..end - lo)
        })
        .collect()
}

/// The maximal runs of `true`.
fn runs_of(live: &[bool]) -> Vec<Range<usize>> {
    let mut runs = Vec::new();
    let mut start = None;
    for (idx, &is_live) in live.iter().enumerate() {
        match (is_live, start) {
            (true, None) => start = Some(idx),
            (false, Some(s)) => {
                runs.push(s..idx);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push(s..live.len());
    }
    runs
}

/// The undecided tail of the read `lo..hi` of `stream` (stream coordinates):
/// the bytes of the live prefix of a switch sequence the read ends in. A prefix
/// is live exactly when completing it makes the bulk extraction drop it as a
/// switch (a prefix whose `ESC` sits in a designator slot stays text, and one
/// on the alternate screen is not main-buffer content either way).
fn undecided_tail(stream: &[u8], lo: usize, hi: usize) -> Range<usize> {
    let from = hi.saturating_sub(7);
    let Some(rel) = stream[from..hi].iter().rposition(|&b| b == ESC) else {
        return 0..0;
    };
    let start = from + rel;
    let suffix = &stream[start..hi];
    for (pattern, _) in PATTERNS {
        if pattern.len() > suffix.len() && pattern.starts_with(suffix) {
            let mut probe = stream[..hi].to_vec();
            probe.extend_from_slice(&pattern[suffix.len()..]);
            let spans = extract_main_buffer(&probe, false, DesignatorSlot::Ground).spans;
            let dropped = (start..hi).all(|k| !spans.iter().any(|r| r.contains(&k)));
            if dropped {
                return start.max(lo)..hi;
            }
        }
    }
    0..0
}

/// What the tracker must return for the read `lo..hi`: the bulk spans
/// restricted to it, without the read's undecided tail.
fn expected_spans(stream: &[u8], bulk: &[Range<usize>], lo: usize, hi: usize) -> Vec<Range<usize>> {
    let mut live = vec![false; hi - lo];
    for r in spans_within(bulk, lo, hi) {
        for k in r {
            live[k] = true;
        }
    }
    for k in undecided_tail(stream, lo, hi) {
        live[k - lo] = false;
    }
    runs_of(&live)
}

/// The bulk extraction's spans of a whole stream, without the empty ranges the
/// extraction records between two back-to-back switches (a range without a
/// byte makes no byte live).
fn bulk_spans(stream: &[u8]) -> Vec<Range<usize>> {
    extract_main_buffer(stream, false, DesignatorSlot::Ground)
        .spans
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect()
}

/// Whether no byte of the read is live.
fn none_live(spans: &[Range<usize>]) -> bool {
    spans.iter().all(|r| r.is_empty())
}

// ---- AC-1 ----

/// AC-1 (FR1, FR3, FR4, FR7; TM-1): for every stream of the tracker corpus and
/// every delivery (every single cut, every pair of cuts, one byte per read),
/// the spans of each read equal the bulk extraction's spans restricted to that
/// read, except that the read's undecided tail is not live, and no check
/// disagrees with the shadow parser.
#[test]
fn altsplit_ac1_every_delivery_of_the_corpus_gives_the_bulk_spans_of_each_read() {
    let mut mismatches: Vec<String> = Vec::new();
    let mut reads_with_a_partial = 0usize;
    let mut checked = 0usize;
    for (name, stream) in tracker_corpus() {
        let bulk = bulk_spans(&stream);
        for offsets in deliveries(stream.len()) {
            let (runs, adoptions) = run_tracker(&stream, &offsets);
            if adoptions != 0 {
                mismatches.push(format!(
                    "{name} cut at {offsets:?}: {adoptions} check(s) disagreed with the shadow parser"
                ));
            }
            for run in &runs {
                checked += 1;
                if run.partial_len > 0 {
                    reads_with_a_partial += 1;
                }
                let expected = expected_spans(&stream, &bulk, run.lo, run.hi);
                if run.spans != expected {
                    mismatches.push(format!(
                        "{name} cut at {offsets:?}: read {}..{}: spans {:?}, expected {:?}",
                        run.lo, run.hi, run.spans, expected
                    ));
                }
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {checked} reads mismatched; first ones: {:#?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(20)]
    );
    assert!(
        reads_with_a_partial > 0,
        "the corpus must leave reads ending inside a switch sequence"
    );
}

/// AC-1 (reference not vacuous): K16's bulk spans start after `L(1049)`, and a
/// read ending inside a switch sequence has its prefix as the undecided tail.
#[test]
fn altsplit_ac1_the_reference_for_k16_is_the_main_screen_after_the_switch_back() {
    let (_, k16) = &sc1x_streams()[0];
    assert_eq!(k16.len(), 26);
    assert_eq!(bulk_spans(k16), vec![16..26]);
    // The read `ESC [ ? 1 0 4 9` (offsets 8..15) ends in a live prefix.
    assert_eq!(undecided_tail(k16, 8, 15), 8..15);
    // The read that follows does not.
    assert_eq!(undecided_tail(k16, 15, 23), 0..0);
    assert_eq!(expected_spans(k16, &bulk_spans(k16), 15, 23), vec![1..8]);
}

// ---- AC-2 ----

/// AC-2 (FR1, FR2, FR7; TM-1): a feed scanner fed each read with the tracker's
/// spans yields the same items in the same order as a scanner fed each read
/// with the bulk spans restricted to that read.
#[test]
fn altsplit_ac2_the_scanner_yields_the_items_of_the_bulk_spans() {
    let mut mismatches: Vec<String> = Vec::new();
    for (name, stream) in tracker_corpus() {
        let bulk = bulk_spans(&stream);
        for offsets in deliveries(stream.len()) {
            let (runs, _) = run_tracker(&stream, &offsets);
            let mut with_tracker = AgentStatusFeedScanner::new();
            let mut with_bulk = AgentStatusFeedScanner::new();
            for run in &runs {
                let read = &stream[run.lo..run.hi];
                let got = with_tracker.feed(read, &run.spans);
                let want = with_bulk.feed(read, &spans_within(&bulk, run.lo, run.hi));
                if got != want {
                    mismatches.push(format!(
                        "{name} cut at {offsets:?}: read {}..{}: items {got:?}, expected {want:?}",
                        run.lo, run.hi
                    ));
                }
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} reads mismatched; first ones: {:#?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(20)]
    );
}

/// AC-2 (FR2): K16's reads (cut at 8, 15, 23) give exactly one OSC 133
/// prompt-start item.
#[test]
fn altsplit_ac2_k16_reads_give_exactly_one_prompt_start_mark() {
    let (_, k16) = &sc1x_streams()[0];
    let (runs, adoptions) = run_tracker(k16, &[8, 15, 23]);
    assert_eq!(adoptions, 0);
    let mut scanner = AgentStatusFeedScanner::new();
    let mut items = Vec::new();
    for run in &runs {
        items.extend(scanner.feed(&k16[run.lo..run.hi], &run.spans));
    }
    let prompt_starts = items.iter().filter(|i| **i == prompt_start()).count();
    assert_eq!(prompt_starts, 1, "items: {items:?}");
}

// ---- AC-4 ----

/// What a run delivered to the client, in delivery order: `(is a snapshot,
/// bytes)`. The last entry of every run is the reader's empty EOF chunk.
fn delivered(run: &SuppressedRun) -> Vec<(bool, Vec<u8>)> {
    run.received
        .iter()
        .map(|c| (c.kind == R2ChunkKind::Snapshot, c.data.clone()))
        .collect()
}

fn chunk(snapshot: bool, data: &[u8]) -> (bool, Vec<u8>) {
    (snapshot, data.to_vec())
}

/// AC-4 (FR5; TM-3): for K16's reads (cut at 8, 15, 23) through the reader with
/// the suppressed-read harness, once with no read suppressed and once with each
/// single read suppressed, the scrollback ring and the chunks delivered to the
/// client are the values the unmodified reader gave for the same runs. The
/// literals below were recorded against the reader before the tracker was
/// wired in. The ring holds none of the third read's bytes.
#[test]
fn altsplit_ac4_k16_reads_leave_the_ring_and_the_delivered_chunks_as_they_were() {
    let (_, k16) = &sc1x_streams()[0];
    let chunks: Vec<Vec<u8>> = [&k16[..8], &k16[8..15], &k16[15..23], &k16[23..]]
        .iter()
        .map(|c| c.to_vec())
        .collect();
    let snapshot_after_enter: &[u8] =
        b"EMSNAP2\x00\x00\x00\x00\x00\x1b[3J\x1b[H\x1b[2J\x1b[?25h\x1b[m\x1b[H\x1b[J\x1b[?1049h";

    let expected: Vec<(&[usize], &[u8], Vec<(bool, Vec<u8>)>)> = vec![
        (
            &[],
            b"\x1b\x1b\\",
            vec![
                chunk(false, b"\x1b[?1049h"),
                chunk(false, b"\x1b[?1049"),
                chunk(false, b"l\x1b]133;A"),
                chunk(false, b"\x1b\x1b\\"),
                chunk(false, b""),
            ],
        ),
        (
            &[0],
            b"\x1b\x1b\\",
            vec![
                chunk(true, snapshot_after_enter),
                chunk(false, b"\x1b[?1049"),
                chunk(false, b"l\x1b]133;A"),
                chunk(false, b"\x1b\x1b\\"),
                chunk(false, b""),
            ],
        ),
        (
            &[1],
            b"\x1b\x1b\\",
            vec![
                chunk(false, b"\x1b[?1049h"),
                chunk(true, snapshot_after_enter),
                chunk(false, b"\x1b[?1049"),
                chunk(false, b"l\x1b]133;A"),
                chunk(false, b"\x1b\x1b\\"),
                chunk(false, b""),
            ],
        ),
        (
            &[2],
            b"\x1b\x1b\\",
            vec![
                chunk(false, b"\x1b[?1049h"),
                chunk(false, b"\x1b[?1049"),
                chunk(
                    true,
                    b"EMSNAP2\x00\x00\x00\x00\x00\x1b[3J\x1b[H\x1b[2J\x1b[?1049l",
                ),
                chunk(false, b"\x1b]133;A"),
                chunk(false, b"\x1b\x1b\\"),
                chunk(false, b""),
            ],
        ),
        (
            &[3],
            b"\x1b\x1b\\",
            vec![
                chunk(false, b"\x1b[?1049h"),
                chunk(false, b"\x1b[?1049"),
                chunk(false, b"l\x1b]133;A"),
                chunk(
                    true,
                    b"EMSNAP2\x00\x01\x00\x00\x00\x00\x00\x00\x00P\x00\x18\x00\x1b[3J\x1b[H\x1b[2J\x1b\x1b\\\x1b[?1049l",
                ),
                chunk(false, b""),
            ],
        ),
    ];

    for (suppressed, ring, chunks_out) in expected {
        let run = run_reader_with_suppressed_reads(&chunks, suppressed);
        assert_eq!(run.ring, ring, "ring with reads {suppressed:?} suppressed");
        assert_eq!(
            delivered(&run),
            chunks_out,
            "chunks delivered with reads {suppressed:?} suppressed"
        );
        assert_eq!(
            r2_count(&run.ring, b"133;A"),
            0,
            "the ring holds none of the third read's bytes (reads {suppressed:?} suppressed)"
        );
    }
}

// ---- edge cases (named so they stay visible) ----

/// Edge case: a read ending in a lone `ESC`. The next read's first byte decides
/// between a switch, a designator introducer and neither.
#[test]
fn altsplit_edge_a_read_ending_in_a_lone_esc_is_decided_by_the_next_reads_first_byte() {
    let mark = cat(&[A, BEL]);

    // A switch back to the main screen: the mark after it is live.
    let mut rig = Rig::new();
    rig.read(&enter(1049));
    assert!(
        none_live(&rig.read(&[ESC])),
        "the lone ESC is an undecided tail"
    );
    assert_eq!(rig.tracker.partial_len(), 1);
    let second = cat(&[b"[?1049l", &mark]);
    assert_eq!(rig.read(&second), vec![7..second.len()]);
    assert_eq!(rig.tracker.partial_len(), 0);

    // A switch to the alternate screen: nothing after it is live.
    let mut rig = Rig::new();
    assert!(none_live(&rig.read(&[ESC])));
    let second = cat(&[b"[?1049h", &mark]);
    assert!(none_live(&rig.read(&second)));
    assert_eq!(rig.tracker.adoptions(), 0);

    // A designator introducer: the `ESC` that follows `(` is the designator,
    // so the switch is text for the client and the mark stays live.
    let mut rig = Rig::new();
    assert!(none_live(&rig.read(&[ESC])));
    let second = cat(&[b"(", &enter(1049), &mark]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 0, "the shadow parser switched too");
    assert_eq!(rig.read(&mark), vec![0..mark.len()]);

    // Neither: the `ESC` was an ordinary escape, its follower an ordinary byte.
    let mut rig = Rig::new();
    assert!(none_live(&rig.read(&[ESC])));
    let second = cat(&[b"x", &mark]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.partial_len(), 0);
}

/// Edge case: a designator slot carried across a read boundary (`ESC (` | the
/// switch, and `ESC ( ESC` | the rest of the switch).
#[test]
fn altsplit_edge_a_designator_slot_is_carried_across_a_read_boundary() {
    let mark = cat(&[A, BEL]);

    let mut rig = Rig::new();
    assert_eq!(rig.read(DS), vec![0..2]);
    let second = cat(&[&enter(1049), &mark]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 0);
    assert_eq!(rig.tracker.partial_len(), 0);

    let mut rig = Rig::new();
    let first = cat(&[DS, &[ESC]]);
    assert_eq!(rig.read(&first), vec![0..3], "the designator is text");
    assert_eq!(rig.tracker.partial_len(), 1);
    let second = cat(&[b"[?1049h", &mark]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 0);

    // On the alternate screen the same bytes stay not live; the leave that
    // follows is a switch.
    let mut rig = Rig::new();
    rig.read(&enter(1049));
    assert!(none_live(&rig.read(DS)));
    let second = cat(&[&leave(1049), &mark]);
    assert!(
        none_live(&rig.read(&second)),
        "a designator-slot leave is text"
    );
    assert_eq!(rig.tracker.adoptions(), 0);
}

/// Edge case: back to the main screen and into the alternate screen again
/// within one read, the first switch completed by that read's first bytes.
#[test]
fn altsplit_edge_back_to_main_and_into_the_alternate_screen_within_one_read() {
    let mark = cat(&[A, BEL]);
    let mut rig = Rig::new();
    rig.read(&cat(&[&enter(1049), b"\x1b[?104"]));
    assert_eq!(rig.tracker.partial_len(), 6);
    let second = cat(&[b"9l", &mark, &enter(1049), &mark]);
    assert_eq!(rig.read(&second), vec![2..2 + mark.len()]);
    assert_eq!(rig.tracker.adoptions(), 0);
    // The pane ends on the alternate screen.
    assert!(none_live(&rig.read(&mark)));
}

// ---- AC-5 ----

/// The properties every `advance` result has, whatever the input (AC-5).
fn assert_well_formed(ctx: &str, read_len: usize, spans: &[Range<usize>], partial_len: usize) {
    assert!(
        partial_len <= 7,
        "{ctx}: carried a partial of {partial_len} bytes"
    );
    let mut prev_end = 0;
    for r in spans {
        assert!(
            r.start <= r.end && r.end <= read_len,
            "{ctx}: span {r:?} lies outside a read of {read_len} bytes"
        );
        assert!(
            r.start >= prev_end,
            "{ctx}: span {r:?} is not after the previous span's end {prev_end}"
        );
        prev_end = r.end;
    }
}

/// AC-5 (NFR1; TM-2): starting on the main and on the alternate screen, every
/// proper prefix of every switch pattern, as a live partial and as a
/// designator partial (after `ESC (`), followed by a read holding each of the
/// 256 byte values: no panic, and every result is well formed.
#[test]
fn altsplit_ac5_every_prefix_followed_by_every_byte_value_is_well_formed() {
    for on_alt in [false, true] {
        for designator in [false, true] {
            for (pattern, _) in PATTERNS {
                for len in 1..pattern.len() {
                    let mut head = Vec::new();
                    if on_alt {
                        head.extend_from_slice(&enter(1049));
                    }
                    if designator {
                        head.extend_from_slice(DS);
                    }
                    head.extend_from_slice(&pattern[..len]);
                    for byte in 0..=u8::MAX {
                        let ctx = format!(
                            "alt={on_alt} designator={designator} prefix={:?} then {byte:#04x}",
                            &pattern[..len]
                        );
                        let mut rig = Rig::new();
                        let spans = rig.read(&head);
                        assert_well_formed(&ctx, head.len(), &spans, rig.tracker.partial_len());
                        let second = [byte];
                        let spans = rig.read(&second);
                        assert_well_formed(&ctx, 1, &spans, rig.tracker.partial_len());
                        // The rest of the pattern and a mark follow.
                        let third = cat(&[&pattern[len..], A, BEL]);
                        let spans = rig.read(&third);
                        assert_well_formed(&ctx, third.len(), &spans, rig.tracker.partial_len());
                    }
                }
            }
        }
    }
}

/// A repeating unit of switch-pattern prefixes, complete patterns, designators
/// and marks, at least 1 MiB when repeated.
fn stress_stream() -> Vec<u8> {
    let mut unit = Vec::new();
    for (pattern, _) in PATTERNS {
        for len in 1..pattern.len() {
            unit.extend_from_slice(&pattern[..len]);
            unit.extend_from_slice(b"ab");
        }
        unit.extend_from_slice(pattern);
        unit.extend_from_slice(A);
        unit.extend_from_slice(BEL);
        unit.extend_from_slice(DS);
        unit.extend_from_slice(pattern);
        unit.extend_from_slice(EE);
    }
    unit.extend_from_slice(X);
    unit.extend_from_slice(b"\x1b[?1;1049h");
    unit.extend_from_slice(&leave(1049));
    unit.extend_from_slice(b"\x1b\x1b\x1b(\x1b(\x1b");
    let mut stream = Vec::new();
    while stream.len() < (1 << 20) + 17 {
        stream.extend_from_slice(&unit);
    }
    stream
}

/// AC-5 (NFR1; TM-2): a stream of at least 1 MiB of switch-pattern prefixes,
/// complete patterns and marks, in 64 KiB reads and (its first 64 KiB) in
/// 1-byte reads, with a real shadow parser: no panic and every result is well
/// formed.
#[test]
fn altsplit_ac5_a_one_mib_stream_is_well_formed_in_64_kib_and_in_one_byte_reads() {
    let stream = stress_stream();
    assert!(stream.len() >= 1 << 20);

    let mut rig = Rig::new();
    for (idx, read) in stream.chunks(64 * 1024).enumerate() {
        let spans = rig.read(read);
        assert_well_formed(
            &format!("64 KiB read {idx}"),
            read.len(),
            &spans,
            rig.tracker.partial_len(),
        );
    }

    let mut rig = Rig::new();
    for (idx, read) in stream[..64 * 1024].chunks(1).enumerate() {
        let spans = rig.read(read);
        assert_well_formed(
            &format!("1-byte read {idx}"),
            read.len(),
            &spans,
            rig.tracker.partial_len(),
        );
    }
}

// ---- differential test against the oracle ----

/// A small deterministic generator (a 64-bit LCG).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
}

/// Random streams over an alphabet built from the switch patterns' bytes, the
/// designator introducers and the mark's bytes, cut at random offsets: whenever
/// no check disagreed with the shadow parser, every read's spans equal the
/// oracle's. (A stream whose switch forms only the shadow parser follows, such
/// as a control byte inside a CSI, takes the fallback and is skipped.)
#[test]
fn altsplit_ac1_random_streams_and_cuts_match_the_oracle() {
    const ALPHABET: &[u8] = b"\x1b\x1b\x1b[[?????1104947lhlh()]A3;\x07\\x";
    let mut rng = Lcg(0x5eed_a17);
    let (mut compared, mut skipped) = (0usize, 0usize);
    for case in 0..4000 {
        let len = 1 + rng.below(40);
        let mut stream: Vec<u8> = (0..len)
            .map(|_| ALPHABET[rng.below(ALPHABET.len())])
            .collect();
        // Seed real switch sequences into some streams so they are not rare.
        if rng.below(2) == 0 {
            let (pattern, _) = PATTERNS[rng.below(PATTERNS.len())];
            let at = rng.below(stream.len());
            stream.splice(at..at, pattern.iter().copied());
        }
        let mut offsets: Vec<usize> = (0..rng.below(4))
            .map(|_| 1 + rng.below(stream.len().max(2) - 1))
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        offsets.retain(|&o| o < stream.len());

        let bulk = bulk_spans(&stream);
        let (runs, adoptions) = run_tracker(&stream, &offsets);
        if adoptions != 0 {
            skipped += 1;
            continue;
        }
        compared += 1;
        for run in &runs {
            let expected = expected_spans(&stream, &bulk, run.lo, run.hi);
            assert_eq!(
                run.spans, expected,
                "case {case}: stream {stream:?} cut at {offsets:?}: read {}..{}",
                run.lo, run.hi
            );
        }
    }
    assert!(
        compared > 2000,
        "too few comparable streams: {compared} compared, {skipped} skipped"
    );
}

// ---- AC-6 ----

/// AC-6 (a) (FR1, A3; TM-1): a form the tracker does not model and the shadow
/// parser follows (`ESC [?1;1049h`) gives a mark after it no live span, the mark
/// in the next read is not live either, and after `ESC [?1049l` in a later read
/// a mark is live again.
#[test]
fn altsplit_ac6a_an_unmodelled_switch_form_keeps_the_alternate_screen_until_a_modelled_leave() {
    let mark = cat(&[A, BEL]);
    let mut rig = Rig::new();

    let first = cat(&[b"\x1b[?1;1049h", &mark]);
    let spans = rig.read(&first);
    assert!(
        rig.parser.screen().alternate_screen(),
        "premise: the shadow parser follows the compound form"
    );
    assert!(
        none_live(&spans),
        "a mark after the unmodelled form is not live: {spans:?}"
    );

    let spans = rig.read(&mark);
    assert!(
        none_live(&spans),
        "a mark in the next read is not live: {spans:?}"
    );

    let spans = rig.read(&leave(1049));
    assert!(
        none_live(&spans),
        "the switch bytes are not live: {spans:?}"
    );
    let spans = rig.read(&mark);
    assert_eq!(spans, vec![0..mark.len()], "a mark after the leave is live");
    assert_eq!(rig.tracker.partial_len(), 0);

    // The same leave and mark in one read.
    let mut rig = Rig::new();
    rig.read(&cat(&[b"\x1b[?1;1049h", &mark]));
    let both = cat(&[&leave(1049), &mark]);
    let spans = rig.read(&both);
    assert_eq!(spans, vec![8..both.len()]);
}

/// AC-6 (b) (FR1): after the tracker has seen `ESC [?1049h` and the shadow
/// parser is then rebuilt (main screen, as the reader does after a parser
/// panic), a mark in the next read is live and no partial is carried into it.
#[test]
fn altsplit_ac6b_a_rebuilt_shadow_parser_resets_the_tracker() {
    let mark = cat(&[A, BEL]);
    let mut rig = Rig::new();
    rig.read(&enter(1049));
    assert!(rig.parser.screen().alternate_screen());

    rig.parser = new_shadow_parser(24, 80);
    let spans = rig.read(&mark);
    assert_eq!(spans, vec![0..mark.len()]);
    assert_eq!(rig.tracker.partial_len(), 0);

    // With a partial pending when the parser is rebuilt, nothing of it is
    // carried into the next read: its `49l` is ordinary text.
    let mut rig = Rig::new();
    rig.read(&cat(&[&enter(1049), b"\x1b[?10"]));
    assert_eq!(rig.tracker.partial_len(), 5);
    rig.parser = new_shadow_parser(24, 80);
    let next = cat(&[b"49l", &mark]);
    let spans = rig.read(&next);
    assert_eq!(spans, vec![0..next.len()]);
    assert_eq!(rig.tracker.partial_len(), 0);
}

/// AC-6 (c): the shadow parser the reader uses enters the alternate screen on
/// `ESC [?1049h` and `ESC [?47h`, leaves it on the matching `l`, and does not
/// act on `ESC [?1047h` / `ESC [?1047l`.
#[test]
fn altsplit_ac6c_the_shadow_parser_follows_1049_and_47_and_ignores_1047() {
    let alt_after = |parser: &mut ShadowParser, bytes: &[u8]| {
        parser.process(bytes);
        parser.screen().alternate_screen()
    };

    for p in [1049u16, 47] {
        let mut parser = new_shadow_parser(24, 80);
        assert!(alt_after(&mut parser, &enter(p)), "?{p}h enters");
        assert!(!alt_after(&mut parser, &leave(p)), "?{p}l leaves");
    }

    let mut parser = new_shadow_parser(24, 80);
    assert!(!alt_after(&mut parser, &enter(1047)), "?1047h is ignored");
    assert!(alt_after(&mut parser, &enter(1049)));
    assert!(
        alt_after(&mut parser, &leave(1047)),
        "?1047l does not leave the alternate screen"
    );
    assert!(!alt_after(&mut parser, &leave(1049)));
}
