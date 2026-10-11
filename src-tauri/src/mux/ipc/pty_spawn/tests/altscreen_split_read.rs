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

// ===== mux-alt-tracker-ris-compound-decset task0001 (FR1-FR8, NFR1-NFR5) =====
//
// The tracker also follows RIS (`ESC c`) and, while its client-rule screen and
// its shadow-rule screen disagree, a DEC private mode set or reset that carries
// 47, 1047 or 1049 in a form outside the six switch patterns.
//
// Test identifiers (task plan): AC-1 RIS in one read, AC-2 split RIS, AC-3 an
// `ESC c` in a designator slot, AC-4 a compound in one read, AC-5 a split
// compound, AC-6 robustness. AC-7 lives in `daemon/tests.rs` (rows K38 to K57
// through the real reader loop and the daemon task); AC-8 is the existing suite
// running unchanged.
//
// Oracle: `extract_main_buffer` does not handle RIS, so the expected spans of
// the RIS streams come from `client_walk` below, a test-side walk of the whole
// stream under the client rule that follows the extraction's rules and treats
// RIS as a return to the main screen whose own bytes keep the liveness of the
// screen before it. It is restricted to each read without the read's undecided
// tail, as the oracle above is.

/// `ESC )`: the byte after it is a charset designator.
const DC: &[u8] = b"\x1b)";
/// RIS.
const R: &[u8] = b"\x1bc";
/// A CSI whose final byte is `c`. It is not RIS.
const CC: &[u8] = b"\x1b[c";
const C1H: &[u8] = b"\x1b[?1;1049h";
const C1L: &[u8] = b"\x1b[?1;1049l";
const CRH: &[u8] = b"\x1b[?1049;1h";
const C7H: &[u8] = b"\x1b[?1;1047h";
const C7L: &[u8] = b"\x1b[?1;1047l";
const C0H: &[u8] = b"\x1b[?01049h";
const CEH: &[u8] = b"\x1b[?;1049h";
/// The start of a DEC private CSI that the RIS right after it interrupts.
const P1: &[u8] = b"\x1b[?1;10";
/// The start of a DEC private CSI (a proper prefix of a switch pattern) that
/// the RIS right after it interrupts.
const P2: &[u8] = b"\x1b[?10";

/// The compound forms of SC-1y, by name.
const COMPOUNDS: [(&str, &[u8]); 7] = [
    ("C1h", C1H),
    ("C1l", C1L),
    ("Crh", CRH),
    ("C7h", C7H),
    ("C7l", C7L),
    ("C0h", C0H),
    ("Ceh", CEH),
];

/// An OSC 133 prompt start with its terminator.
fn mark() -> Vec<u8> {
    cat(&[A, BEL])
}

/// M1: the client rule on the alternate screen, the shadow parser on the main
/// screen (it ignores ?1047).
fn m1() -> Vec<u8> {
    enter(1047)
}

/// M2: the client rule on the main screen (the switch's `ESC` is a
/// designator), the shadow parser on the alternate screen.
fn m2() -> Vec<u8> {
    cat(&[DS, &enter(1049)])
}

/// The two mismatched starting states, by name.
fn mismatched_setups() -> [(&'static str, Vec<u8>); 2] {
    [("M1", m1()), ("M2", m2())]
}

/// Whether the compound ends with `l` (it returns to the main screen).
fn leaves(compound: &[u8]) -> bool {
    compound.last() == Some(&b'l')
}

/// The per-byte result of [`client_walk`].
struct Walk {
    /// Whether the byte is main-buffer content under the client rule.
    live: Vec<bool>,
    /// Whether the byte is an `ESC` outside a designator slot.
    live_esc: Vec<bool>,
}

/// Walk a whole stream under the client rule: a recognized switch pattern
/// switches screens (its bytes are not content) unless its `ESC` sits in the
/// designator slot right after `ESC (` / `ESC )`; `ESC c` outside a slot is a
/// RIS (back to the main screen, its own bytes keep the liveness of the screen
/// before it). Compound forms are ordinary bytes, except that `screen_after`
/// `(index, alt)` sets the screen right after the byte at `index`: it stands for
/// the compound a detection read applies.
fn client_walk(stream: &[u8], screen_after: Option<(usize, bool)>) -> Walk {
    let points: Vec<(usize, bool)> = screen_after.into_iter().collect();
    client_walk_points(stream, &points).walk
}

/// The result of [`client_walk_points`].
struct PointWalk {
    walk: Walk,
    /// The screen under the client rule right after the walk has passed the
    /// byte (`true`: the alternate screen). Inside a run of bytes the walk
    /// takes as a unit (a switch pattern, `ESC c`, `ESC (` with its designator)
    /// it is the screen before the run, until the run's last byte.
    alt_after: Vec<bool>,
}

/// [`client_walk`] with any number of screen override points: for each
/// `(index, alt)` of `points`, the screen is `alt` right after the byte at
/// `index` (a point stands for the compound a detection read applies, or for the
/// parser's state a cross-check read adopts). A point inside a run of bytes
/// that the walk takes as a unit has no effect.
fn client_walk_points(stream: &[u8], points: &[(usize, bool)]) -> PointWalk {
    let n = stream.len();
    let mut live = vec![false; n];
    let mut live_esc = vec![false; n];
    let mut alt_after = vec![false; n];
    let mut alt = false;
    let set = |i: usize, alt: &mut bool| {
        for &(at, to) in points {
            if at == i {
                *alt = to;
            }
        }
    };
    let mut i = 0usize;
    while i < n {
        if stream[i] != ESC {
            live[i] = !alt;
            set(i, &mut alt);
            alt_after[i] = alt;
            i += 1;
            continue;
        }
        live_esc[i] = true;
        if let Some(&(pattern, is_enter)) =
            PATTERNS.iter().find(|(p, _)| stream[i..].starts_with(p))
        {
            let before = alt;
            alt = is_enter;
            for k in i..i + pattern.len() {
                alt_after[k] = if k + 1 == i + pattern.len() {
                    alt
                } else {
                    before
                };
            }
            i += pattern.len();
            continue;
        }
        live[i] = !alt;
        match stream.get(i + 1) {
            Some(b'c') => {
                live[i + 1] = !alt;
                alt_after[i] = alt;
                alt = false;
                alt_after[i + 1] = alt;
                i += 2;
            }
            Some(b'(' | b')') => {
                // The byte after the introducer is the designator, text
                // whatever it is.
                for k in i + 1..(i + 3).min(n) {
                    live[k] = !alt;
                }
                for k in i..(i + 3).min(n) {
                    alt_after[k] = alt;
                }
                i += 3;
            }
            _ => {
                set(i, &mut alt);
                alt_after[i] = alt;
                i += 1;
            }
        }
    }
    PointWalk {
        walk: Walk { live, live_esc },
        alt_after,
    }
}

/// The undecided tail of the read `lo..hi` under [`client_walk`]: the bytes of a
/// proper prefix of a switch pattern the read ends in, when its `ESC` is not in
/// a designator slot.
fn walk_tail(stream: &[u8], walk: &Walk, lo: usize, hi: usize) -> Range<usize> {
    let from = hi.saturating_sub(7);
    let Some(rel) = stream[from..hi].iter().rposition(|&b| b == ESC) else {
        return 0..0;
    };
    let start = from + rel;
    if !walk.live_esc[start] {
        return 0..0;
    }
    let suffix = &stream[start..hi];
    if PATTERNS
        .iter()
        .any(|(p, _)| p.len() > suffix.len() && p.starts_with(suffix))
    {
        start.max(lo)..hi
    } else {
        0..0
    }
}

/// What the tracker must return for the read `lo..hi`: the walk's live bytes
/// restricted to it, without the read's undecided tail.
fn walk_spans(stream: &[u8], walk: &Walk, lo: usize, hi: usize) -> Vec<Range<usize>> {
    let mut live = walk.live[lo..hi].to_vec();
    for k in walk_tail(stream, walk, lo, hi) {
        live[k - lo] = false;
    }
    runs_of(&live)
}

/// Every delivery of `stream` through a [`Rig`]: for each, exactly
/// `expected_adoptions` checks disagree with the shadow parser and each read's
/// spans equal `expected(lo, hi)`. Returns the mismatches.
fn check_deliveries(
    name: &str,
    stream: &[u8],
    expected_adoptions: u32,
    expected: &dyn Fn(usize, usize) -> Vec<Range<usize>>,
) -> Vec<String> {
    let mut mismatches = Vec::new();
    for offsets in deliveries(stream.len()) {
        let (runs, adoptions) = run_tracker(stream, &offsets);
        if adoptions != expected_adoptions {
            mismatches.push(format!(
                "{name} cut at {offsets:?}: {adoptions} adoption(s), expected {expected_adoptions}"
            ));
        }
        for run in &runs {
            let want = expected(run.lo, run.hi);
            if run.spans != want {
                mismatches.push(format!(
                    "{name} cut at {offsets:?}: read {}..{}: spans {:?}, expected {want:?}",
                    run.lo, run.hi, run.spans
                ));
            }
        }
    }
    mismatches
}

fn assert_no_mismatches(mismatches: &[String]) {
    assert!(
        mismatches.is_empty(),
        "{} mismatches; first ones: {:#?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(20)]
    );
}

/// Oracle sanity: where a stream holds no RIS in a position the bulk
/// extraction cannot model, the walk's liveness equals the bulk extraction's
/// for every stream of the tracker corpus.
#[test]
fn altmismatch_the_client_walk_agrees_with_the_bulk_extraction_on_the_corpus() {
    for (name, stream) in tracker_corpus() {
        let walk = client_walk(&stream, None);
        let mut bulk_live = vec![false; stream.len()];
        for r in bulk_spans(&stream) {
            for k in r {
                bulk_live[k] = true;
            }
        }
        assert_eq!(walk.live, bulk_live, "{name}");
    }
}

/// The AC-1 streams: a RIS and a mark after each starting state.
fn ris_streams() -> Vec<(&'static str, Vec<u8>)> {
    let mark = mark();
    vec![
        ("main", cat(&[R, &mark])),
        ("E(1049)", cat(&[&enter(1049), R, &mark])),
        ("M1", cat(&[&m1(), R, &mark])),
        (
            "E(1049) DS L(1049)",
            cat(&[&enter(1049), DS, &leave(1049), R, &mark]),
        ),
        ("M2", cat(&[&m2(), R, &mark])),
    ]
}

/// The streams in which a RIS interrupts a DEC private CSI or a carried partial
/// switch.
fn ris_interrupt_streams() -> Vec<(&'static str, Vec<u8>)> {
    let mark = mark();
    vec![
        ("M1 P1 R", cat(&[&m1(), P1, R, &mark])),
        ("M1 P2 R", cat(&[&m1(), P2, R, &mark])),
        ("main P1 R", cat(&[P1, R, &mark])),
        ("main P2 R", cat(&[P2, R, &mark])),
        ("M2 P1 R", cat(&[&m2(), P1, R, &mark])),
        ("M2 P2 R", cat(&[&m2(), P2, R, &mark])),
    ]
}

// ---- AC-1 ----

/// AC-1 (FR1; TM-2): a read of R followed by A BEL gives a live mark after
/// each starting state, a mark in a later read is live, no check disagrees with
/// the shadow parser, and on the main screen the live run is not interrupted at
/// R.
#[test]
fn altmismatch_ac1_a_ris_in_one_read_gives_a_live_mark_in_every_starting_state() {
    let expected: [(&str, Vec<Range<usize>>); 5] = [
        ("main", vec![0..10]),
        ("E(1049)", vec![10..18]),
        ("M1", vec![10..18]),
        ("E(1049) DS L(1049)", vec![20..28]),
        ("M2", vec![0..20]),
    ];
    for ((name, stream), (expected_name, literal)) in ris_streams().into_iter().zip(expected) {
        assert_eq!(name, expected_name);
        let walk = client_walk(&stream, None);
        let (runs, adoptions) = run_tracker(&stream, &[]);
        assert_eq!(adoptions, 0, "{name}");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].spans, literal, "{name}: the read's spans");
        assert_eq!(
            runs[0].spans,
            walk_spans(&stream, &walk, 0, stream.len()),
            "{name}: the oracle"
        );
        assert!(
            runs[0]
                .spans
                .iter()
                .any(|r| r.contains(&(stream.len() - 1))),
            "{name}: the mark's BEL is live"
        );

        // A mark in a later read is live and no adoption occurs.
        let mut rig = Rig::new();
        rig.read(&stream);
        assert_eq!(rig.read(&mark()), vec![0..8], "{name}: a later mark");
        assert_eq!(rig.tracker.adoptions(), 0, "{name}");
    }
}

/// AC-1 (FR1; TM-2): the feed scanner fed the tracker's spans yields a reset
/// and then the prompt-start mark for every starting state.
#[test]
fn altmismatch_ac1_the_scanner_yields_the_reset_and_a_live_mark() {
    for (name, stream) in ris_streams() {
        let (runs, _) = run_tracker(&stream, &[]);
        let mut scanner = AgentStatusFeedScanner::new();
        let items = scanner.feed(&stream, &runs[0].spans);
        assert_eq!(
            items,
            vec![AgentStatusFeedItem::Reset, prompt_start()],
            "{name}"
        );
    }
}

// ---- AC-2 ----

/// AC-2 (FR2; TM-2): every AC-1 stream with every single cut, every pair of
/// cuts and one byte per read gives the oracle's spans on each read, with no
/// adoption.
#[test]
fn altmismatch_ac2_every_delivery_of_the_ris_streams_gives_the_oracle_spans() {
    for (name, stream) in ris_streams().into_iter().chain(ris_interrupt_streams()) {
        let walk = client_walk(&stream, None);
        let mismatches = check_deliveries(name, &stream, 0, &|lo, hi| {
            walk_spans(&stream, &walk, lo, hi)
        });
        assert_no_mismatches(&mismatches);
    }
}

/// AC-2 (FR2; TM-2): the scanner fed the tracker's spans yields the same items
/// as one fed the oracle's spans, in every delivery of the RIS streams.
#[test]
fn altmismatch_ac2_the_scanner_yields_the_items_of_the_oracle_spans() {
    for (name, stream) in ris_streams().into_iter().chain(ris_interrupt_streams()) {
        let walk = client_walk(&stream, None);
        for offsets in deliveries(stream.len()) {
            let (runs, _) = run_tracker(&stream, &offsets);
            let mut with_tracker = AgentStatusFeedScanner::new();
            let mut with_oracle = AgentStatusFeedScanner::new();
            for run in &runs {
                let read = &stream[run.lo..run.hi];
                let got = with_tracker.feed(read, &run.spans);
                let want = with_oracle.feed(read, &walk_spans(&stream, &walk, run.lo, run.hi));
                assert_eq!(
                    got, want,
                    "{name} cut at {offsets:?}: read {}..{}",
                    run.lo, run.hi
                );
            }
        }
    }
}

/// AC-2 (FR2; TM-2): a cut between `ESC` and `c` still gives a RIS.
#[test]
fn altmismatch_ac2_a_cut_between_esc_and_c_still_gives_a_ris() {
    let mut rig = Rig::new();
    rig.read(&m1());
    assert!(none_live(&rig.read(&[ESC])), "the lone ESC is undecided");
    assert_eq!(rig.tracker.partial_len(), 1);
    let second = cat(&[b"c", &mark()]);
    assert_eq!(rig.read(&second), vec![1..second.len()]);
    assert_eq!(rig.tracker.partial_len(), 0);
    assert_eq!(rig.read(&mark()), vec![0..8], "a later mark is live");
    assert_eq!(rig.tracker.adoptions(), 0);

    // On the main screen the `c` itself stays live.
    let mut rig = Rig::new();
    assert!(none_live(&rig.read(&[ESC])));
    let second = cat(&[b"c", &mark()]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 0);
}

/// AC-2 (FR2; TM-2): M1 CC A BEL cut at `ESC [ | c` gives no RIS: the mark
/// stays not live, and a mark in a later read is not live either.
#[test]
fn altmismatch_ac2_a_csi_cut_between_the_bracket_and_c_is_not_a_ris() {
    let stream = cat(&[&m1(), CC, &mark()]);
    let mismatches = check_deliveries("M1 CC A BEL", &stream, 0, &|_, _| Vec::new());
    assert_no_mismatches(&mismatches);

    let mut rig = Rig::new();
    rig.read(&m1());
    assert!(none_live(&rig.read(b"\x1b[")));
    assert_eq!(rig.tracker.partial_len(), 2);
    let second = cat(&[b"c", &mark()]);
    assert!(
        none_live(&rig.read(&second)),
        "no RIS: the mark is not live"
    );
    assert!(none_live(&rig.read(&mark())), "a later mark is not live");
    assert_eq!(rig.tracker.adoptions(), 0);
}

/// AC-2 (FR2; TM-2): `ESC [?10 | ESC c` and `ESC [?1;10 | ESC c`, each after
/// M1, give a RIS: the mark after it is live.
#[test]
fn altmismatch_ac2_a_ris_after_a_carried_partial_or_parse_is_a_ris() {
    for (name, head) in [("ESC [?10", P2), ("ESC [?1;10", P1)] {
        let mut rig = Rig::new();
        rig.read(&cat(&[&m1(), head]));
        let second = cat(&[R, &mark()]);
        assert_eq!(
            rig.read(&second),
            vec![2..second.len()],
            "{name}: the mark after the RIS is live"
        );
        assert_eq!(rig.tracker.adoptions(), 0, "{name}");
        assert_eq!(rig.read(&mark()), vec![0..8], "{name}: a later mark");
    }
}

/// AC-2 (FR2, FR5; TM-2): a RIS ends a DEC private CSI parse: the bytes after
/// it are text (no detection), and the screens are both the main screen.
#[test]
fn altmismatch_ac2_a_ris_clears_the_csi_parse() {
    let mut rig = Rig::new();
    rig.read(&cat(&[&m1(), P1]));
    let second = cat(&[R, b"49h", &mark()]);
    assert_eq!(rig.read(&second), vec![2..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 0);
    assert_eq!(rig.tracker.partial_len(), 0);
}

// ---- AC-3 ----

/// AC-3 (FR3; TM-2): an `ESC c` in a designator slot on the alternate screen
/// (`E(1049) DS R A BEL`, `E(1049) DC R A BEL`), in one read and with every
/// single cut, pair of cuts and one byte per read: the mark is not live, a
/// mark in a later read is not live, and no adoption occurs.
#[test]
fn altmismatch_ac3_an_esc_c_in_a_designator_slot_is_not_a_ris_for_the_client() {
    for (name, introducer) in [("DS", DS), ("DC", DC)] {
        let stream = cat(&[&enter(1049), introducer, R, &mark()]);
        let mismatches = check_deliveries(name, &stream, 0, &|_, _| Vec::new());
        assert_no_mismatches(&mismatches);

        let mut rig = Rig::new();
        rig.read(&stream);
        assert!(none_live(&rig.read(&mark())), "{name}: a later mark");
        assert_eq!(rig.tracker.adoptions(), 0, "{name}");
    }
}

/// AC-3 (FR3; TM-2): the cuts that matter most (`ESC ( | ESC c` and
/// `ESC ( ESC | c`) keep the client on the alternate screen and the shadow-rule
/// screen on the main screen, so the parser's state matches.
#[test]
fn altmismatch_ac3_the_slot_cuts_leave_the_client_on_the_alternate_screen() {
    for cut_inside in [false, true] {
        let mut rig = Rig::new();
        rig.read(&cat(&[&enter(1049), DS]));
        let third = if cut_inside {
            // `ESC ( ESC | c`.
            assert!(none_live(&rig.read(&[ESC])));
            cat(&[b"c", &mark()])
        } else {
            // `ESC ( | ESC c`.
            cat(&[R, &mark()])
        };
        assert!(none_live(&rig.read(&third)));
        assert!(!rig.parser.screen().alternate_screen());
        assert_eq!(rig.tracker.adoptions(), 0);
        assert!(none_live(&rig.read(&mark())));
    }
}

/// AC-3 (FR3; pin for SPEC A1): the shadow parser ends on the main screen after
/// `E(1049) DS R`, and after a RIS in every other starting state.
#[test]
fn altmismatch_ac3_the_shadow_parser_ends_on_the_main_screen_after_a_ris() {
    let alt_after = |bytes: &[u8]| {
        let mut parser = new_shadow_parser(24, 80);
        parser.process(bytes);
        parser.screen().alternate_screen()
    };
    assert!(alt_after(&enter(1049)), "premise: E(1049) enters");
    assert!(!alt_after(&cat(&[&enter(1049), DS, R])), "E(1049) DS R");
    assert!(!alt_after(&cat(&[&enter(1049), DC, R])), "E(1049) DC R");
    assert!(!alt_after(&cat(&[&enter(1049), R])), "E(1049) R");
    assert!(alt_after(&m2()), "premise: M2 is on the alternate screen");
    assert!(!alt_after(&cat(&[&m2(), R])), "M2 R");
    assert!(!alt_after(&cat(&[&m1(), R])), "M1 R");
}

// ---- AC-4 ----

/// AC-4 (FR4; TM-2): after a mismatched setup (M1, M2), each compound is
/// delivered as three reads (the setup, a read holding the compound and A BEL,
/// a read holding A BEL). The second read's spans are the whole read for a
/// final `l` and none for a final `h`; exactly one adoption is counted; the
/// third read's mark is live exactly when the final byte is `l`.
#[test]
fn altmismatch_ac4_a_compound_after_a_mismatch_is_detected_in_its_read() {
    for (setup_name, setup) in mismatched_setups() {
        for (name, compound) in COMPOUNDS {
            let ctx = format!("{setup_name} {name}");
            let mut rig = Rig::new();
            rig.read(&setup);
            assert_eq!(rig.tracker.adoptions(), 0, "{ctx}: the setup");

            let second = cat(&[compound, &mark()]);
            let spans = rig.read(&second);
            let expected = if leaves(compound) {
                vec![0..second.len()]
            } else {
                Vec::new()
            };
            assert_eq!(spans, expected, "{ctx}: the detection read");
            assert_eq!(rig.tracker.adoptions(), 1, "{ctx}");
            assert_eq!(rig.tracker.partial_len(), 0, "{ctx}");

            let third = rig.read(&mark());
            let expected = if leaves(compound) {
                vec![0..8]
            } else {
                Vec::new()
            };
            assert_eq!(third, expected, "{ctx}: the third read");
            assert_eq!(rig.tracker.adoptions(), 1, "{ctx}: no further adoption");
        }
    }
}

/// AC-4 (FR4, A2, FR8; TM-2): in a matched state (E(1049)) nothing is detected.
/// C1l gives the existing fallback (no span) with one adoption and the third
/// read's mark is live; every other compound gives no span, no adoption and a
/// third-read mark that is not live.
#[test]
fn altmismatch_ac4_a_compound_in_a_matched_state_keeps_todays_behavior() {
    for (name, compound) in COMPOUNDS {
        let mut rig = Rig::new();
        rig.read(&enter(1049));
        let second = cat(&[compound, &mark()]);
        assert!(none_live(&rig.read(&second)), "{name}: the second read");
        let is_c1l = name == "C1l";
        assert_eq!(rig.tracker.adoptions(), u32::from(is_c1l), "{name}");
        let third = rig.read(&mark());
        if is_c1l {
            assert_eq!(third, vec![0..8], "{name}: the third read");
        } else {
            assert!(none_live(&third), "{name}: the third read");
        }
    }
}

/// AC-4 (FR4, A7; TM-2): a compound whose `ESC` sits in a designator slot is
/// never detected: one adoption is counted exactly when the shadow parser
/// changes screen (M1 then DS C1h, M2 then DS C1l), and the third read's mark is
/// live exactly after M2.
#[test]
fn altmismatch_ac4_a_compound_in_a_designator_slot_is_not_detected() {
    for (setup_name, setup) in mismatched_setups() {
        for (name, compound) in [("C1h", C1H), ("C1l", C1L)] {
            let ctx = format!("{setup_name} DS {name}");
            let mut rig = Rig::new();
            rig.read(&setup);
            let second = cat(&[DS, compound, &mark()]);
            let spans = rig.read(&second);
            let parser_changes =
                (setup_name == "M1" && name == "C1h") || (setup_name == "M2" && name == "C1l");
            assert_eq!(rig.tracker.adoptions(), u32::from(parser_changes), "{ctx}");
            let expected = if setup_name == "M2" && !parser_changes {
                vec![0..second.len()]
            } else {
                Vec::new()
            };
            assert_eq!(spans, expected, "{ctx}: the second read");
            let third = rig.read(&mark());
            if setup_name == "M2" {
                assert_eq!(third, vec![0..8], "{ctx}: the third read");
            } else {
                assert!(none_live(&third), "{ctx}: the third read");
            }
        }
    }
}

/// AC-4 (SPEC A1, TS3): the shadow parser follows C1h and ignores C7h (and the
/// other forms the expectations rest on).
#[test]
fn altmismatch_ac4_the_shadow_parser_follows_c1h_and_ignores_c7h() {
    let alt_after = |parser: &mut ShadowParser, bytes: &[u8]| {
        parser.process(bytes);
        parser.screen().alternate_screen()
    };
    for (name, compound, enters) in [
        ("C1h", C1H, true),
        ("Crh", CRH, true),
        ("C0h", C0H, true),
        ("Ceh", CEH, true),
        ("C7h", C7H, false),
    ] {
        let mut parser = new_shadow_parser(24, 80);
        assert_eq!(alt_after(&mut parser, compound), enters, "{name}");
    }
    let mut parser = new_shadow_parser(24, 80);
    assert!(alt_after(&mut parser, &enter(1049)));
    assert!(!alt_after(&mut parser, C1L), "C1l leaves");
    assert!(alt_after(&mut parser, &enter(1049)));
    assert!(alt_after(&mut parser, C7L), "C7l does not leave");
}

/// AC-4 (FR4; TM-2; superseded in part by mux-alt-tracker-detection-read-boundary
/// FR2/FR6): a compound followed by a switch pattern in the same read, and a
/// detection read that ends in a partial switch (which is kept for the next
/// read, because it comes from bytes after the last detected compound).
#[test]
fn altmismatch_ac4_a_detection_read_applies_later_switches_and_keeps_the_later_carry() {
    // M1 then `C1l E(1049)`: the compound returns to the main screen, the
    // switch pattern enters the alternate screen again: nothing is live.
    let mut rig = Rig::new();
    rig.read(&m1());
    assert!(none_live(&rig.read(&cat(&[C1L, &enter(1049), &mark()]))));
    assert_eq!(rig.tracker.adoptions(), 1);
    assert!(none_live(&rig.read(&mark())));

    // M1 then `C1h L(1049)`: the other way round, the read ends on main.
    let mut rig = Rig::new();
    rig.read(&m1());
    let second = cat(&[C1H, &leave(1049), &mark()]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 1);
    assert_eq!(rig.read(&mark()), vec![0..8]);

    // M1 then `C1l ESC [?10`: the partial switch after the compound is kept
    // and the next read completes it as L(1049).
    let mut rig = Rig::new();
    rig.read(&m1());
    let second = cat(&[C1L, P2]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 1);
    assert_eq!(rig.tracker.partial_len(), 5);
    let third = cat(&[b"49l", &mark()]);
    assert_eq!(rig.read(&third), vec![3..11], "`49l` completes L(1049)");

    // M1 then `C1l ESC`: a lone ESC at the end is kept too.
    let mut rig = Rig::new();
    rig.read(&m1());
    assert_eq!(rig.read(&cat(&[C1L, &[ESC]])), vec![0..11]);
    assert_eq!(rig.tracker.partial_len(), 1);
}

// ---- AC-5 ----

/// AC-5 (FR5; TM-2): each mismatched AC-4 case as one stream (M1 or M2, the
/// compound, A BEL, A BEL), with every single cut, every pair of cuts and one
/// byte per read: exactly one adoption; the read that holds the compound's final
/// byte gives the whole read for a final `l` and no span for a final `h`; the
/// reads before it keep today's spans; every mark completed in a later read is
/// live exactly when the final byte is `l`.
#[test]
fn altmismatch_ac5_every_delivery_of_a_split_compound_is_detected_once() {
    for (setup_name, setup) in mismatched_setups() {
        for (name, compound) in COMPOUNDS {
            let ctx = format!("{setup_name} {name}");
            let stream = cat(&[&setup, compound, &mark(), &mark()]);
            let final_idx = setup.len() + compound.len() - 1;
            let l = leaves(compound);
            let walk = client_walk(&stream, Some((final_idx, !l)));
            let mismatches = check_deliveries(&ctx, &stream, 1, &|lo, hi| {
                if lo <= final_idx && final_idx < hi {
                    if l { vec![0..hi - lo] } else { Vec::new() }
                } else {
                    walk_spans(&stream, &walk, lo, hi)
                }
            });
            assert_no_mismatches(&mismatches);
        }
    }
}

/// AC-5 (FR5; TM-2): `ESC [?104 | 9;1h` and `ESC [?1049 | ;1h` are among the
/// detected cuts, in both mismatched directions.
#[test]
fn altmismatch_ac5_a_partial_switch_that_turns_into_a_compound_is_detected() {
    for (setup_name, setup) in mismatched_setups() {
        for (head, tail) in [(&b"\x1b[?104"[..], &b"9;1h"[..]), (b"\x1b[?1049", b";1h")] {
            let ctx = format!("{setup_name} {head:?} | {tail:?}");
            let mut rig = Rig::new();
            rig.read(&setup);
            assert!(none_live(&rig.read(head)), "{ctx}: the undecided tail");
            assert_eq!(rig.tracker.adoptions(), 0, "{ctx}: nothing yet");
            let second = cat(&[tail, &mark()]);
            assert!(none_live(&rig.read(&second)), "{ctx}: a final `h`");
            assert_eq!(rig.tracker.adoptions(), 1, "{ctx}");
            assert!(none_live(&rig.read(&mark())), "{ctx}: a later mark");
        }
    }
}

/// AC-5 (FR5; TM-2): after M1 and after M2, a DEC private CSI that is aborted
/// or that is not a set or reset gives no detection and no adoption, and every
/// read's spans are those of the tracker without the CSI parse (the oracle).
#[test]
fn altmismatch_ac5_aborted_or_other_csi_forms_are_not_detected() {
    let inputs: Vec<(&str, Vec<u8>)> = vec![
        ("?1;10 CAN 49h", cat(&[P1, &[0x18], b"49h"])),
        ("?1;10 SUB 49h", cat(&[P1, &[0x1a], b"49h"])),
        ("?1;10 ESC[0m 49h", cat(&[P1, EX, b"49h"])),
        ("?1049$p", b"\x1b[?1049$p".to_vec()),
        ("?1;1049m", b"\x1b[?1;1049m".to_vec()),
    ];
    for (setup_name, setup) in mismatched_setups() {
        for (name, input) in &inputs {
            let ctx = format!("{setup_name} {name}");
            let stream = cat(&[&setup, input, &mark()]);
            let walk = client_walk(&stream, None);
            let mismatches = check_deliveries(&ctx, &stream, 0, &|lo, hi| {
                walk_spans(&stream, &walk, lo, hi)
            });
            assert_no_mismatches(&mismatches);
        }
    }
}

// ---- differential test against the oracle ----

/// Random streams over an alphabet that holds `c`, the designator introducers,
/// the switch patterns' bytes and the mark's bytes, cut at random offsets:
/// whenever no check disagreed with the shadow parser, every read's spans equal
/// the oracle's (the walk handles RIS and designator slots; a detection counts
/// as an adoption and is skipped, as the existing differential test skips it).
#[test]
fn altmismatch_ac2_random_streams_with_ris_match_the_oracle() {
    const ALPHABET: &[u8] = b"\x1b\x1b\x1bcc[[?????1104947lhlh()]A3;\x07\\x";
    let mut rng = Lcg(0x715_c0de);
    let (mut compared, mut skipped, mut with_ris) = (0usize, 0usize, 0usize);
    for case in 0..6000 {
        let len = 1 + rng.below(40);
        let mut stream: Vec<u8> = (0..len)
            .map(|_| ALPHABET[rng.below(ALPHABET.len())])
            .collect();
        if rng.below(2) == 0 {
            let (pattern, _) = PATTERNS[rng.below(PATTERNS.len())];
            let at = rng.below(stream.len());
            stream.splice(at..at, pattern.iter().copied());
        }
        if rng.below(2) == 0 {
            let at = rng.below(stream.len());
            stream.splice(at..at, R.iter().copied());
        }
        let mut offsets: Vec<usize> = (0..rng.below(4))
            .map(|_| 1 + rng.below(stream.len().max(2) - 1))
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        offsets.retain(|&o| o < stream.len());

        let walk = client_walk(&stream, None);
        let (runs, adoptions) = run_tracker(&stream, &offsets);
        if adoptions != 0 {
            skipped += 1;
            continue;
        }
        compared += 1;
        if stream.windows(2).any(|w| w == R) {
            with_ris += 1;
        }
        for run in &runs {
            let expected = walk_spans(&stream, &walk, run.lo, run.hi);
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
    assert!(with_ris > 500, "too few streams with a RIS: {with_ris}");
}

// ---- AC-6 ----

/// The starting states of AC-6: a matched state and both mismatched directions.
fn robustness_setups() -> [(&'static str, Vec<u8>); 3] {
    [("matched", enter(1049)), ("M1", m1()), ("M2", m2())]
}

/// AC-6 (NFR1; TM-1): in a matched state and in both mismatched directions,
/// every prefix of C1h, Crh, C7l, C0h and Ceh followed by a read holding each of
/// the 256 byte values, then the rest and A BEL: no panic, every result is well
/// formed and the carried partial switch never exceeds 7 bytes.
#[test]
fn altmismatch_ac6_every_compound_prefix_followed_by_every_byte_value_is_well_formed() {
    for (setup_name, setup) in robustness_setups() {
        for (name, compound) in [
            ("C1h", C1H),
            ("Crh", CRH),
            ("C7l", C7L),
            ("C0h", C0H),
            ("Ceh", CEH),
        ] {
            for len in 1..compound.len() {
                let head = cat(&[&setup, &compound[..len]]);
                for byte in 0..=u8::MAX {
                    let ctx = format!("{setup_name} {name}[..{len}] then {byte:#04x}");
                    let mut rig = Rig::new();
                    let spans = rig.read(&head);
                    assert_well_formed(&ctx, head.len(), &spans, rig.tracker.partial_len());
                    let spans = rig.read(&[byte]);
                    assert_well_formed(&ctx, 1, &spans, rig.tracker.partial_len());
                    let third = cat(&[&compound[len..], A, BEL]);
                    let spans = rig.read(&third);
                    assert_well_formed(&ctx, third.len(), &spans, rig.tracker.partial_len());
                }
            }
        }
    }
}

/// `ESC [?` followed by `params` and `h`.
fn dec_set(params: &str) -> Vec<u8> {
    format!("\x1b[?{params}h").into_bytes()
}

/// AC-6 (NFR1; TM-1): oversized parameters. A parameter of more than 20 digits,
/// parameters whose decimal value is 1049 plus a multiple of 65536 or of 2^32
/// (none reads as 1049), and more than 1000 parameters give no panic and a well
/// formed result, in every delivery shape (one read, every single cut, one byte
/// per read), in a matched state and in both mismatched directions.
#[test]
fn altmismatch_ac6_oversized_parameters_are_well_formed_and_never_read_as_1049() {
    let many_ones = "1;".repeat(1001);
    let not_1049: Vec<(String, Vec<u8>)> = vec![
        ("23 nines".into(), dec_set(&"9".repeat(23))),
        ("60 digits".into(), dec_set(&"1".repeat(60))),
        (
            "1049 + 65536".into(),
            dec_set(&(1049u64 + 65536).to_string()),
        ),
        (
            "1049 + 3 * 65536".into(),
            dec_set(&(1049u64 + 3 * 65536).to_string()),
        ),
        (
            "1049 + 2^32".into(),
            dec_set(&(1049u64 + (1 << 32)).to_string()),
        ),
        (
            "1049 + 5 * 2^32".into(),
            dec_set(&(1049u64 + 5 * (1 << 32)).to_string()),
        ),
        (
            "1047 + 2^32 after 1".into(),
            dec_set(&format!("1;{}", 1047u64 + (1 << 32))),
        ),
        ("1001 parameters".into(), dec_set(&format!("{many_ones}1"))),
    ];
    for (setup_name, setup) in robustness_setups() {
        for (name, form) in &not_1049 {
            let stream = cat(&[&setup, form, &mark()]);
            for offsets in deliveries_sample(stream.len()) {
                let ctx = format!("{setup_name} {name} cut at {offsets:?}");
                let (runs, adoptions) = run_tracker(&stream, &offsets);
                assert_eq!(adoptions, 0, "{ctx}: none of them reads as 1049");
                for run in &runs {
                    assert_well_formed(&ctx, run.hi - run.lo, &run.spans, run.partial_len);
                }
            }
        }
        // Leading zeros do not change the value: more than 20 digits that read
        // as 1049 are detected after a mismatch and ignored otherwise; and a
        // 1049 beyond the 1000th parameter is detected (SPEC A6).
        let leading_zeros = dec_set(&format!("{}1049", "0".repeat(25)));
        let beyond = dec_set(&format!("{many_ones}1049"));
        for (name, form) in [
            ("25 leading zeros", &leading_zeros),
            ("1049 beyond 1000", &beyond),
        ] {
            let stream = cat(&[&setup, form, &mark()]);
            for offsets in deliveries_sample(stream.len()) {
                let ctx = format!("{setup_name} {name} cut at {offsets:?}");
                let (runs, adoptions) = run_tracker(&stream, &offsets);
                if setup_name != "matched" {
                    assert_eq!(adoptions, 1, "{ctx}");
                }
                for run in &runs {
                    assert_well_formed(&ctx, run.hi - run.lo, &run.spans, run.partial_len);
                }
            }
        }
    }
}

/// A few deliveries of a long stream: in one read, every cut of its first 40
/// bytes, cuts across the rest in steps, and one byte per read.
fn deliveries_sample(len: usize) -> Vec<Vec<usize>> {
    let mut all = vec![Vec::new()];
    for o in 1..len.min(40) {
        all.push(vec![o]);
    }
    let step = (len / 23).max(1);
    all.push((1..len).step_by(step).collect());
    all.push((1..len).collect());
    all
}

/// A repeating unit of the forms AC-6 names: compounds, their prefixes broken
/// off by a junk byte, RIS in every position, designator slots, long and many
/// parameters, aborting bytes, switch patterns and marks.
fn robustness_unit() -> Vec<u8> {
    let mut unit = Vec::new();
    for (_, compound) in COMPOUNDS {
        unit.extend_from_slice(compound);
        unit.extend_from_slice(A);
        unit.push(0x07);
        for len in 1..compound.len() {
            unit.extend_from_slice(&compound[..len]);
            unit.extend_from_slice(b"ab");
        }
    }
    for junk in [0x18u8, 0x1a, 0x1b, b'x', b'$', b':', b'<'] {
        unit.extend_from_slice(P1);
        unit.push(junk);
        unit.extend_from_slice(b"49h");
    }
    unit.extend_from_slice(P1);
    unit.extend_from_slice(R);
    unit.extend_from_slice(P2);
    unit.extend_from_slice(R);
    unit.extend_from_slice(DS);
    unit.extend_from_slice(R);
    unit.extend_from_slice(DC);
    unit.extend_from_slice(R);
    unit.extend_from_slice(CC);
    unit.extend_from_slice(&dec_set(&"1;".repeat(1001)));
    unit.extend_from_slice(&dec_set(&"9".repeat(30)));
    unit.extend_from_slice(&dec_set(&(1049u64 + 65536).to_string()));
    unit.extend_from_slice(&dec_set(&(1049u64 + (1 << 32)).to_string()));
    for (pattern, _) in PATTERNS {
        for len in 1..pattern.len() {
            unit.extend_from_slice(&pattern[..len]);
            unit.extend_from_slice(b"ab");
        }
        unit.extend_from_slice(pattern);
        unit.extend_from_slice(A);
        unit.push(0x07);
        unit.extend_from_slice(DS);
        unit.extend_from_slice(pattern);
    }
    unit.extend_from_slice(b"\x1b\x1b\x1b(\x1b(\x1b\x1bc\x1b");
    unit
}

/// AC-6 (NFR1; TM-1): a stream of at least 1 MiB of these forms, in 64 KiB
/// reads and in 1-byte reads, with a real shadow parser, in a matched state and
/// in both mismatched directions: no panic, every result is well formed, and the
/// carried partial switch never exceeds 7 bytes.
#[test]
fn altmismatch_ac6_a_one_mib_stream_is_well_formed_in_64_kib_and_in_one_byte_reads() {
    let unit = robustness_unit();
    let mut stream = Vec::new();
    while stream.len() < (1 << 20) + 17 {
        stream.extend_from_slice(&unit);
    }
    assert!(stream.len() >= 1 << 20);

    for (setup_name, setup) in robustness_setups() {
        for read_len in [64 * 1024, 1] {
            let mut rig = Rig::new();
            let spans = rig.read(&setup);
            assert_well_formed("setup", setup.len(), &spans, rig.tracker.partial_len());
            for (idx, read) in stream.chunks(read_len).enumerate() {
                let spans = rig.read(read);
                assert_well_formed(
                    &format!("{setup_name} {read_len}-byte reads, read {idx}"),
                    read.len(),
                    &spans,
                    rig.tracker.partial_len(),
                );
            }
        }
    }
}

// ---- mux-alt-tracker-detection-read-boundary task0001 (FR1-FR3, FR6-FR8, NFR1;
// TM-1, TM-2, TM-3) ----
//
// A detection read detects every compound that follows the first detected one
// in the same read, whatever its two screens say, and keeps the carried item
// that the scan recorded after the last detected compound. Notation of the
// task plan: M1, M2, C1h, C1l, Crh, C7h, C7l, C0h, Ceh, DS, DC, R, P1 and P2 are
// the constants and helpers above; A BEL is `mark()`.
//
// Oracle (AC-3): `detect_oracle` walks the reads of one delivery in order and
// keeps the tracker's two screens. The shadow-rule screen follows the shadow
// rule (the ?1049 and ?47 patterns and RIS, also in a designator slot) and takes
// the parser's state after every detection read and every cross-check read. The
// client-rule screen is the walk of the whole stream under the client rule with
// one override point for every detected compound and one for every cross-check
// read ([`client_walk_points`]).

/// A stream built from parts that remembers where each compound ends.
struct DetectStream {
    bytes: Vec<u8>,
    /// The index of each compound's terminator, and whether it is an `h`.
    compounds: Vec<(usize, bool)>,
}

impl DetectStream {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            compounds: Vec::new(),
        }
    }

    /// Append bytes that hold no compound.
    fn push(mut self, part: &[u8]) -> Self {
        self.bytes.extend_from_slice(part);
        self
    }

    /// Append one compound (its last byte is the terminator).
    fn compound(mut self, part: &[u8]) -> Self {
        self.bytes.extend_from_slice(part);
        self.compounds.push((self.bytes.len() - 1, !leaves(part)));
        self
    }
}

/// The shadow rule's screen changes in a whole stream: `(index of the last byte,
/// screen after)` for each ?1049 and ?47 pattern and each `ESC c` (a RIS), also
/// in a designator slot (every `ESC` aborts what came before it).
fn shadow_events(stream: &[u8]) -> Vec<(usize, bool)> {
    let mut events = Vec::new();
    for i in 0..stream.len() {
        if stream[i] != ESC {
            continue;
        }
        if let Some(&(pattern, is_enter)) =
            PATTERNS.iter().find(|(p, _)| stream[i..].starts_with(p))
        {
            if !pattern.starts_with(b"\x1b[?1047") {
                events.push((i + pattern.len() - 1, is_enter));
            }
        } else if stream.get(i + 1) == Some(&b'c') {
            events.push((i + 1, false));
        }
    }
    events
}

/// The shadow-rule screen right before the byte at `t`, for a read that starts
/// at `lo` with the shadow-rule screen `start`.
fn shadow_before(events: &[(usize, bool)], lo: usize, start: bool, t: usize) -> bool {
    events
        .iter()
        .rev()
        .find(|&&(last, _)| lo <= last && last < t)
        .map_or(start, |&(_, screen)| screen)
}

/// The per-delivery oracle of the task plan (Test Notes): every read's spans and
/// the number of adoptions of `stream` delivered cut at `offsets`.
fn detect_oracle(stream: &DetectStream, offsets: &[usize]) -> (Vec<Vec<Range<usize>>>, u32) {
    let bytes = &stream.bytes;
    let events = shadow_events(bytes);
    let mut parser = new_shadow_parser(24, 80);
    // The override points of the client-rule walk.
    let mut points: Vec<(usize, bool)> = Vec::new();
    // The shadow-rule screen at the start of the read.
    let mut shadow = false;
    let mut adoptions = 0u32;
    // Per read: its spans, or `None` for a read of rule 3, decided once every
    // override point is known.
    let mut reads: Vec<(usize, usize, Option<Vec<Range<usize>>>)> = Vec::new();
    let mut lo = 0usize;
    for &hi in offsets.iter().chain(std::iter::once(&bytes.len())) {
        let before = parser.screen().alternate_screen();
        parser.process(&bytes[lo..hi]);
        let after = parser.screen().alternate_screen();

        // Rule 1: the compounds the read detects.
        let mut detected = false;
        for &(t, enters) in stream.compounds.iter().filter(|&&(t, _)| lo <= t && t < hi) {
            let client_alt = client_walk_points(bytes, &points).alt_after[t - 1];
            if detected || client_alt != shadow_before(&events, lo, shadow, t) {
                detected = true;
                points.push((t, enters));
            }
        }
        let shadow_end = shadow_before(&events, lo, shadow, hi);
        let spans = if detected {
            adoptions += 1;
            let alt = client_walk_points(bytes, &points).alt_after[hi - 1];
            Some(if alt { Vec::new() } else { vec![0..hi - lo] })
        } else if shadow_end != after {
            // Rule 2: the post-read cross-check.
            adoptions += 1;
            points.push((hi - 1, after));
            Some(if !before && !after {
                vec![0..hi - lo]
            } else {
                Vec::new()
            })
        } else {
            // Rule 3.
            None
        };
        shadow = if spans.is_some() { after } else { shadow_end };
        reads.push((lo, hi, spans));
        lo = hi;
    }
    let walk = client_walk_points(bytes, &points).walk;
    let spans = reads
        .into_iter()
        .map(|(lo, hi, spans)| spans.unwrap_or_else(|| walk_spans(bytes, &walk, lo, hi)))
        .collect();
    (spans, adoptions)
}

/// Every delivery of `stream` (one read, every single cut, every pair of cuts,
/// one byte per read) through a [`Rig`]: each read's spans and the adoption count
/// equal the oracle's. Returns the mismatches.
fn check_detect_deliveries(name: &str, stream: &DetectStream) -> Vec<String> {
    let mut mismatches = Vec::new();
    for offsets in deliveries(stream.bytes.len()) {
        let (runs, adoptions) = run_tracker(&stream.bytes, &offsets);
        let (want, want_adoptions) = detect_oracle(stream, &offsets);
        if adoptions != want_adoptions {
            mismatches.push(format!(
                "{name} cut at {offsets:?}: {adoptions} adoption(s), expected {want_adoptions}"
            ));
        }
        for (run, want) in runs.iter().zip(&want) {
            if run.spans != *want {
                mismatches.push(format!(
                    "{name} cut at {offsets:?}: read {}..{}: spans {:?}, expected {want:?}",
                    run.lo, run.hi, run.spans
                ));
            }
        }
    }
    mismatches
}

/// The streams of AC-1 for AC-3: every pair of compounds after M1 (an l-form,
/// then an h-form) and after M2 (an h-form, then an l-form), and the two triples,
/// each followed by A BEL A BEL.
fn detect_pair_streams() -> Vec<(String, DetectStream)> {
    let marks = cat(&[&mark(), &mark()]);
    let mut all = Vec::new();
    for (setup_name, setup, first_leaves) in [("M1", m1(), true), ("M2", m2(), false)] {
        for &(first_name, first) in COMPOUNDS
            .iter()
            .filter(|&&(_, c)| leaves(c) == first_leaves)
        {
            for &(second_name, second) in COMPOUNDS
                .iter()
                .filter(|&&(_, c)| leaves(c) != first_leaves)
            {
                all.push((
                    format!("{setup_name} {first_name} {second_name}"),
                    DetectStream::new()
                        .push(&setup)
                        .compound(first)
                        .compound(second)
                        .push(&marks),
                ));
            }
        }
    }
    all.push((
        "M1 C1l C1h C1l".into(),
        DetectStream::new()
            .push(&m1())
            .compound(C1L)
            .compound(C1H)
            .compound(C1L)
            .push(&marks),
    ));
    all.push((
        "M2 C1h C1l C1h".into(),
        DetectStream::new()
            .push(&m2())
            .compound(C1H)
            .compound(C1L)
            .compound(C1H)
            .push(&marks),
    ));
    all
}

/// The AC-2 cases that hold no designator introducer, each written as one stream
/// (the setup and both reads joined), followed by A BEL.
fn detect_carry_streams() -> Vec<(String, DetectStream)> {
    let marks = cat(&[&mark(), &mark()]);
    vec![
        (
            "M1 C1l P2 47h A BEL".into(),
            DetectStream::new()
                .push(&m1())
                .compound(C1L)
                .push(P2)
                .push(b"47h")
                .push(&marks),
        ),
        (
            "M1 C7h P2 49l A BEL".into(),
            DetectStream::new()
                .push(&m1())
                .compound(C7H)
                .push(P2)
                .push(b"49l")
                .push(&marks),
        ),
        (
            "M1 C7h P1 49l A BEL".into(),
            DetectStream::new()
                .push(&m1())
                .compound(C7H)
                .compound(&cat(&[P1, b"49l"]))
                .push(&marks),
        ),
        (
            "M1 C7h ESC c A BEL".into(),
            DetectStream::new()
                .push(&m1())
                .compound(C7H)
                .push(R)
                .push(&marks),
        ),
    ]
}

/// The AC-2 cases that hold a designator introducer (`slot`: DS or DC), each
/// written as one stream, followed by A BEL.
fn detect_slot_streams(slot_name: &str, slot: &[u8]) -> Vec<(String, DetectStream)> {
    let marks = cat(&[&mark(), &mark()]);
    vec![
        (
            format!("M1 C7h {slot_name} R A BEL"),
            DetectStream::new()
                .push(&m1())
                .compound(C7H)
                .push(slot)
                .push(R)
                .push(&marks),
        ),
        (
            format!("M1 C7h {slot_name} P2 49h A BEL"),
            DetectStream::new()
                .push(&m1())
                .compound(C7H)
                .push(slot)
                .push(P2)
                .push(b"49h")
                .push(&marks),
        ),
    ]
}

// ---- AC-1 ----

/// AC-1 (FR1, FR3; TM-1), reproduction 1: M1, then `C1l C1h A BEL` in one read.
/// The second read has no span and counts one adoption, and a third read of
/// A BEL has no span.
#[test]
fn altdetect_ac1_reproduction_1_an_l_form_and_an_h_form_after_m1_are_both_detected() {
    let mut rig = Rig::new();
    rig.read(&m1());
    let second = cat(&[C1L, C1H, &mark()]);
    assert!(none_live(&rig.read(&second)), "the second read");
    assert_eq!(rig.tracker.adoptions(), 1);
    assert!(none_live(&rig.read(&mark())), "the third read");
    assert_eq!(rig.tracker.adoptions(), 1, "no further adoption");
}

/// AC-1 (FR1, FR3; TM-1), reverse: M2, then `C1h C1l A BEL` in one read. The
/// second read is the whole read with one adoption, and a third read of A BEL
/// gives 0..8.
#[test]
fn altdetect_ac1_the_reverse_an_h_form_and_an_l_form_after_m2_are_both_detected() {
    let mut rig = Rig::new();
    rig.read(&m2());
    let second = cat(&[C1H, C1L, &mark()]);
    assert_eq!(rig.read(&second), vec![0..second.len()], "the second read");
    assert_eq!(rig.tracker.adoptions(), 1);
    assert_eq!(rig.read(&mark()), vec![0..8], "the third read");
    assert_eq!(rig.tracker.adoptions(), 1, "no further adoption");
}

/// AC-1 (FR1, FR3; TM-1): every pair of compounds in one read followed by A BEL
/// counts one adoption. After M1, each l-form followed by each h-form gives no
/// span and a later mark is not live; after M2, each h-form followed by each
/// l-form gives the whole read and a later mark is live.
#[test]
fn altdetect_ac1_every_pair_of_compounds_in_one_read_counts_one_adoption() {
    for (setup_name, setup, first_leaves) in [("M1", m1(), true), ("M2", m2(), false)] {
        for &(first_name, first) in COMPOUNDS
            .iter()
            .filter(|&&(_, c)| leaves(c) == first_leaves)
        {
            for &(second_name, second) in COMPOUNDS
                .iter()
                .filter(|&&(_, c)| leaves(c) != first_leaves)
            {
                let ctx = format!("{setup_name} {first_name} {second_name}");
                let mut rig = Rig::new();
                rig.read(&setup);
                let read = cat(&[first, second, &mark()]);
                let spans = rig.read(&read);
                let later = rig.read(&mark());
                if first_leaves {
                    assert!(none_live(&spans), "{ctx}: the pair's read");
                    assert!(none_live(&later), "{ctx}: a later mark");
                } else {
                    assert_eq!(spans, vec![0..read.len()], "{ctx}: the pair's read");
                    assert_eq!(later, vec![0..8], "{ctx}: a later mark");
                }
                assert_eq!(rig.tracker.adoptions(), 1, "{ctx}");
            }
        }
    }
}

/// AC-1 (FR1, FR3; TM-1): three alternating compounds in one read count one
/// adoption. After M1, `C1l C1h C1l A BEL` gives the whole read and a later mark
/// is live; after M2, `C1h C1l C1h A BEL` gives no span and a later mark is not
/// live.
#[test]
fn altdetect_ac1_three_alternating_compounds_in_one_read_count_one_adoption() {
    let mut rig = Rig::new();
    rig.read(&m1());
    let read = cat(&[C1L, C1H, C1L, &mark()]);
    assert_eq!(rig.read(&read), vec![0..read.len()], "M1 C1l C1h C1l");
    assert_eq!(rig.tracker.adoptions(), 1);
    assert_eq!(rig.read(&mark()), vec![0..8], "a later mark is live");
    assert_eq!(rig.tracker.adoptions(), 1);

    let mut rig = Rig::new();
    rig.read(&m2());
    let read = cat(&[C1H, C1L, C1H, &mark()]);
    assert!(none_live(&rig.read(&read)), "M2 C1h C1l C1h");
    assert_eq!(rig.tracker.adoptions(), 1);
    assert!(none_live(&rig.read(&mark())), "a later mark is not live");
    assert_eq!(rig.tracker.adoptions(), 1);
}

// ---- AC-2 ----

/// AC-2 (FR2, FR3; TM-1), reproduction 2: after M1, `C7h DS | R A BEL`. The
/// second read has no span, the same as one read of the same bytes, and adds no
/// adoption.
#[test]
fn altdetect_ac2_reproduction_2_the_designator_wait_after_a_detection_read_is_kept() {
    let mut whole = Rig::new();
    whole.read(&m1());
    let one_read = whole.read(&cat(&[C7H, DS, R, &mark()]));
    assert!(none_live(&one_read), "one read of the same bytes");
    assert_eq!(whole.tracker.adoptions(), 1);

    let mut rig = Rig::new();
    rig.read(&m1());
    assert!(none_live(&rig.read(&cat(&[C7H, DS]))));
    assert_eq!(rig.tracker.adoptions(), 1);
    let second = rig.read(&cat(&[R, &mark()]));
    assert!(
        none_live(&second),
        "the `ESC c` sits in the designator slot"
    );
    assert_eq!(second, one_read);
    assert_eq!(rig.tracker.adoptions(), 1, "the second read adds none");
    assert!(none_live(&rig.read(&mark())), "a later mark is not live");
}

/// AC-2 (FR2, FR3; TM-1): after M1, `C1l P2 | 47h A BEL`. The first read is the
/// whole read with a partial length of 5. The partial completes ?1047h, so the
/// second read has no span, and a later mark is not live.
#[test]
fn altdetect_ac2_a_kept_partial_completes_a_pattern_that_enters_the_alternate_screen() {
    let mut rig = Rig::new();
    rig.read(&m1());
    let first = cat(&[C1L, P2]);
    assert_eq!(rig.read(&first), vec![0..first.len()]);
    assert_eq!(rig.tracker.partial_len(), 5);
    assert_eq!(rig.tracker.adoptions(), 1);
    let second = cat(&[b"47h", &mark()]);
    assert!(
        none_live(&rig.read(&second)),
        "the client is on the alternate screen"
    );
    assert_eq!(rig.tracker.partial_len(), 0);
    assert!(none_live(&rig.read(&mark())), "a later mark is not live");
    assert_eq!(rig.tracker.adoptions(), 1);
}

/// AC-2 (FR2, FR3; TM-1): after M1, `C7h P2 | 49l A BEL`. The partial completes
/// L(1049), and the mark in the second read is live.
#[test]
fn altdetect_ac2_a_kept_partial_completes_a_pattern_that_leaves_the_alternate_screen() {
    let mut rig = Rig::new();
    rig.read(&m1());
    let first = cat(&[C7H, P2]);
    assert!(none_live(&rig.read(&first)));
    assert_eq!(rig.tracker.partial_len(), 5);
    let second = cat(&[b"49l", &mark()]);
    assert_eq!(rig.read(&second), vec![3..second.len()], "the mark is live");
    assert_eq!(rig.tracker.partial_len(), 0);
    assert_eq!(rig.read(&mark()), vec![0..8], "a later mark is live");
    assert_eq!(rig.tracker.adoptions(), 1);
}

/// AC-2 (FR2, FR3; TM-1): after M1, `C7h P1 | 49l A BEL`. The carried CSI parse
/// ends in a mismatched state and is detected in the second read. That read is
/// the whole read, with two adoptions in total.
#[test]
fn altdetect_ac2_a_kept_csi_parse_is_detected_in_the_next_read() {
    let mut rig = Rig::new();
    rig.read(&m1());
    assert!(none_live(&rig.read(&cat(&[C7H, P1]))));
    assert_eq!(rig.tracker.adoptions(), 1);
    let second = cat(&[b"49l", &mark()]);
    assert_eq!(rig.read(&second), vec![0..second.len()]);
    assert_eq!(rig.tracker.adoptions(), 2);
    assert_eq!(rig.read(&mark()), vec![0..8], "a later mark is live");
    assert_eq!(rig.tracker.adoptions(), 2);
}

/// AC-2 (FR2, FR3; TM-1): after M1, `C7h ESC | c A BEL`. The partial length is 1
/// after the first read. The second read is a RIS, and its mark is live.
#[test]
fn altdetect_ac2_a_kept_lone_esc_and_the_next_c_are_a_ris() {
    let mut rig = Rig::new();
    rig.read(&m1());
    assert!(none_live(&rig.read(&cat(&[C7H, &[ESC]]))));
    assert_eq!(rig.tracker.partial_len(), 1);
    let second = cat(&[b"c", &mark()]);
    assert_eq!(rig.read(&second), vec![1..second.len()], "the mark is live");
    assert_eq!(rig.tracker.partial_len(), 0);
    assert_eq!(rig.read(&mark()), vec![0..8], "a later mark is live");
    assert_eq!(rig.tracker.adoptions(), 1);
}

/// AC-2 (FR2, FR3; TM-1): after M1, `C7h DS P2 | 49h A BEL` (and with DC). The
/// completed pattern sits in the designator slot. The second read has no span,
/// and the total stays at one adoption.
#[test]
fn altdetect_ac2_a_kept_designator_partial_completes_in_the_slot() {
    for (name, slot) in [("DS", DS), ("DC", DC)] {
        let mut rig = Rig::new();
        rig.read(&m1());
        assert!(none_live(&rig.read(&cat(&[C7H, slot, P2]))), "{name}");
        assert_eq!(rig.tracker.partial_len(), 5, "{name}");
        let second = cat(&[b"49h", &mark()]);
        assert!(none_live(&rig.read(&second)), "{name}: the second read");
        assert_eq!(rig.tracker.adoptions(), 1, "{name}");
        assert!(none_live(&rig.read(&mark())), "{name}: a later mark");
        assert_eq!(rig.tracker.adoptions(), 1, "{name}");
    }
}

// ---- AC-3 ----

/// AC-3 (FR1, FR3; TM-1): the oracle is not vacuous. For `M1 C1l C1h A BEL A BEL`
/// it gives no live byte and one adoption in one read, and two adoptions when a
/// cut separates the compounds (the second one is judged in a matched state, and
/// the post-read cross-check adopts the parser's alternate screen).
#[test]
fn altdetect_ac3_the_oracle_gives_the_literal_results_of_two_deliveries() {
    let marks = cat(&[&mark(), &mark()]);
    let stream = DetectStream::new()
        .push(&m1())
        .compound(C1L)
        .compound(C1H)
        .push(&marks);
    assert_eq!(stream.bytes.len(), 44);

    let (spans, adoptions) = detect_oracle(&stream, &[]);
    assert_eq!(spans, vec![Vec::<Range<usize>>::new()]);
    assert_eq!(adoptions, 1);

    // M1 C1l | C1h A BEL A BEL.
    let (spans, adoptions) = detect_oracle(&stream, &[18]);
    assert_eq!(spans, vec![vec![0..18], Vec::<Range<usize>>::new()]);
    assert_eq!(adoptions, 2);

    // Three override points in one read: the walk ends on the main screen after
    // M1 C1l C1h C1l, and on the alternate screen after M2 C1h C1l C1h.
    let streams = detect_pair_streams();
    for (name, whole_read) in [("M1 C1l C1h C1l", true), ("M2 C1h C1l C1h", false)] {
        let (_, stream) = streams
            .iter()
            .find(|(candidate, _)| candidate == name)
            .expect(name);
        let (spans, adoptions) = detect_oracle(stream, &[]);
        let expected = if whole_read {
            vec![vec![0..stream.bytes.len()]]
        } else {
            vec![Vec::<Range<usize>>::new()]
        };
        assert_eq!(spans, expected, "{name}");
        assert_eq!(adoptions, 1, "{name}");
    }
}

/// AC-3 (FR1, FR2, FR3; TM-1): the adoption count differs by delivery. `M1 C1l
/// C1h A BEL` counts one adoption when both compounds share a read and two when
/// a cut separates them.
#[test]
fn altdetect_ac3_the_adoption_count_differs_by_delivery() {
    let stream = cat(&[&m1(), C1L, C1H, &mark()]);
    let (_, adoptions) = run_tracker(&stream, &[]);
    assert_eq!(adoptions, 1, "one read");
    let (_, adoptions) = run_tracker(&stream, &[18]);
    assert_eq!(adoptions, 2, "a cut between the compounds");
}

/// AC-3 (FR1, FR3; TM-1): every pair and triple of AC-1, each followed by A BEL
/// A BEL, in every delivery (one read, every single cut, every pair of cuts, one
/// byte per read): every read's spans and the adoption count equal the
/// per-delivery oracle.
#[test]
fn altdetect_ac3_every_delivery_of_the_compound_pairs_and_triples_matches_the_oracle() {
    let streams = detect_pair_streams();
    assert_eq!(
        streams.len(),
        22,
        "ten pairs after each setup and two triples"
    );
    let mut mismatches = Vec::new();
    for (name, stream) in &streams {
        mismatches.extend(check_detect_deliveries(name, stream));
    }
    assert_no_mismatches(&mismatches);
}

/// AC-3 (FR2, FR3; TM-1): every AC-2 case written as one stream followed by
/// A BEL, and the designator cases again with DC, in every delivery: every
/// read's spans and the adoption count equal the per-delivery oracle.
#[test]
fn altdetect_ac3_every_delivery_of_the_carry_streams_matches_the_oracle() {
    let mut streams = detect_carry_streams();
    streams.extend(detect_slot_streams("DS", DS));
    streams.extend(detect_slot_streams("DC", DC));
    assert_eq!(streams.len(), 8);
    let mut mismatches = Vec::new();
    for (name, stream) in &streams {
        mismatches.extend(check_detect_deliveries(name, stream));
    }
    assert_no_mismatches(&mismatches);
}

// ---- AC-4 ----

/// `text` with the comment markers of every line removed and the whitespace
/// collapsed to single spaces.
fn prose(text: &str) -> String {
    let stripped: Vec<&str> = text
        .lines()
        .map(|line| {
            line.trim_start()
                .trim_start_matches("///")
                .trim_start_matches("//")
        })
        .collect();
    stripped
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The prose of `source` from the first occurrence of `from` up to the first
/// occurrence of `to` after it.
fn prose_between(source: &str, from: &str, to: &str) -> String {
    let start = source
        .find(from)
        .unwrap_or_else(|| panic!("write_filter.rs has no {from:?}"));
    let rest = &source[start..];
    let end = rest
        .find(to)
        .unwrap_or_else(|| panic!("write_filter.rs has no {to:?} after {from:?}"));
    prose(&rest[..end])
}

/// AC-4 (FR7): the three doc comments of Design section 5 (the tracker's own,
/// the detection branch of the per-read entry point and the scan's) state that
/// detection continues for every later compound in the same read and that only
/// the state recorded after the last detected compound is carried. They no
/// longer say that a detection clears every carried item or that the tracker
/// acts on a compound only in a mismatched state.
#[test]
fn altdetect_ac4_the_doc_comments_state_the_new_behavior() {
    let source = include_str!("../write_filter.rs");

    let tracker = prose_between(
        source,
        "/// Split-aware live spans for the agent-status feed scanner.",
        "pub(super) struct AltSpanTracker",
    );
    assert!(
        tracker.contains("detection continues for every later compound of the read"),
        "the tracker's doc comment: detection continues: {tracker}"
    );
    assert!(
        tracker.contains("Only the state recorded after the last detected compound is carried"),
        "the tracker's doc comment: the later state is carried: {tracker}"
    );
    assert!(
        !tracker.contains("every carried item is cleared"),
        "the tracker's doc comment no longer says a detection clears the carry: {tracker}"
    );
    assert!(
        !tracker.contains("acts on it only in a mismatched state"),
        "the tracker's doc comment no longer says it acts only in a mismatched state: {tracker}"
    );

    let entry = prose_between(
        source,
        "// A detection read: the spans are the fallback",
        "self.shadow_alt = shadow_alt_after;",
    );
    assert!(
        entry.contains("is kept for the next read"),
        "the entry point's comment: the carried item is kept: {entry}"
    );
    assert!(
        !entry.contains("nothing is carried"),
        "the entry point's comment no longer says nothing is carried: {entry}"
    );

    let scan = prose_between(
        source,
        "/// Scan `read` from its start, resolving the carried item first.",
        "fn scan(",
    );
    assert!(
        scan.contains("so does every later one in the same read"),
        "the scan's doc comment: every later compound is detected: {scan}"
    );
    assert!(
        scan.contains("follows the last detected compound"),
        "the scan's doc comment: the carried item follows the last compound: {scan}"
    );
    assert!(
        !scan.contains("ended in a mismatched state)"),
        "the scan's doc comment no longer ties detection to the first compound alone: {scan}"
    );
}

/// AC-4 (FR6): the predecessor's record lists the renamed test under its new
/// full name in AC-4, carries the supersede note as a YAML comment, and keeps
/// its `red_reason`.
#[test]
fn altdetect_ac4_the_predecessor_record_carries_the_new_name_and_the_supersede_note() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test-docs/mux-alt-tracker-ris-compound-decset/task0001.tests.yaml");
    let record = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let start = record
        .find("\n  AC-4:\n")
        .expect("the record has an AC-4 block")
        + 1;
    let rest = &record[start..];
    let end = rest
        .find("\n  AC-5:\n")
        .expect("the record has an AC-5 block");
    let block = &rest[..end];

    let tests: Vec<&str> = block
        .lines()
        .filter_map(|line| line.strip_prefix("      - "))
        .collect();
    let renamed = "mux::ipc::pty_spawn::tests::altscreen_split_read::\
altmismatch_ac4_a_detection_read_applies_later_switches_and_keeps_the_later_carry";
    assert!(
        tests.contains(&renamed),
        "AC-4 lists the new name: {tests:?}"
    );
    assert!(
        !tests
            .iter()
            .any(|name| name.ends_with("_and_clears_the_carry")),
        "AC-4 no longer lists the old name: {tests:?}"
    );
    assert_eq!(tests.len(), 5, "{tests:?}");

    assert!(
        block.contains(
            "    # Superseded: the expectation \"a detection read clears its carried item\" \
is replaced by feature-docs/mux-alt-tracker-detection-read-boundary/SPEC.md FR2/FR6.\n"
        ),
        "AC-4 carries the supersede note as a YAML comment: {block}"
    );
    assert!(block.contains("    red_confirmed: true\n"));
    assert!(
        block.contains(
            "    red_reason: \"Against the unmodified tracker, \
altmismatch_ac4_a_compound_after_a_mismatch_is_detected_in_its_read failed with"
        ),
        "AC-4 keeps its red_reason: {block}"
    );
}

// ---- AC-6 ----

/// AC-6 (NFR1; TM-2): after M1 and after M2, a read holding C1h, C1l, C7h or C7l
/// followed by `ESC`, DS, DC, P2, P1 or DS P2, then a read of one byte (each of
/// the 256 values), then A BEL: no panic, every result is well formed, and the
/// carried partial switch never exceeds 7 bytes.
#[test]
fn altdetect_ac6_a_detection_read_followed_by_every_byte_value_is_well_formed() {
    let tails: [(&str, Vec<u8>); 6] = [
        ("ESC", vec![ESC]),
        ("DS", DS.to_vec()),
        ("DC", DC.to_vec()),
        ("P2", P2.to_vec()),
        ("P1", P1.to_vec()),
        ("DS P2", cat(&[DS, P2])),
    ];
    for (setup_name, setup) in mismatched_setups() {
        for (name, compound) in [("C1h", C1H), ("C1l", C1L), ("C7h", C7H), ("C7l", C7L)] {
            for (tail_name, tail) in &tails {
                let head = cat(&[compound, tail]);
                for byte in 0..=u8::MAX {
                    let ctx = format!("{setup_name} {name} {tail_name} then {byte:#04x}");
                    let mut rig = Rig::new();
                    let spans = rig.read(&setup);
                    assert_well_formed(&ctx, setup.len(), &spans, rig.tracker.partial_len());
                    let spans = rig.read(&head);
                    assert_well_formed(&ctx, head.len(), &spans, rig.tracker.partial_len());
                    let spans = rig.read(&[byte]);
                    assert_well_formed(&ctx, 1, &spans, rig.tracker.partial_len());
                    let last = mark();
                    let spans = rig.read(&last);
                    assert_well_formed(&ctx, last.len(), &spans, rig.tracker.partial_len());
                }
            }
        }
    }
}
