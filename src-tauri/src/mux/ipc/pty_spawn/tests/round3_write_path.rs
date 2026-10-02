//! mux-suppressed-output-round3-fixes task0002: write-path client parity —
//! constructs a removed or straddling screen switch closed (FR1, FR4) and the
//! byte after `ESC (` / `ESC )` as a designator in every path (FR2, FR3).
//!
//! The module sees the top-level helpers of `pty_spawn/tests.rs` (reader
//! drivers, `r2_*` oracle helpers) through its parent; it does not edit them
//! or the inline `fr8_snapshot_tail` module, so the visibility-restore driver
//! used by the FR1 reader-level cases is written here.

use super::*;
use crate::mux::scrollback_filter::{
    strip_pty_output_for_scrollback_write, strip_pty_output_for_scrollback_write_with_designator,
    strip_replayable_rich_content, strip_rich_content_and_remap,
    strip_rich_content_and_remap_with_designator,
};
use crate::mux::session::pane::{AnyPermit, ResumeOutcome, resume_pane_with_permit};
use term_core::terminal_core::{ReplaySegment, TerminalCore};

pub(super) const DIMS: (u16, u16) = (80, 24);

/// The three removed-switch pairs: 47, 1047 and 1049 `h` / `l`.
pub(super) fn switch_pairs() -> [(&'static [u8], &'static [u8]); 3] {
    [
        (b"\x1b[?47h", b"\x1b[?47l"),
        (b"\x1b[?1047h", b"\x1b[?1047l"),
        (b"\x1b[?1049h", b"\x1b[?1049l"),
    ]
}

// ── filter helpers ───────────────────────────────────────────────────────

/// What a filter emitted and still holds after being fed in pieces.
#[derive(Debug, PartialEq)]
struct FilterResult {
    emitted: Vec<u8>,
    pending: Vec<u8>,
    awaiting: bool,
}

fn feed_pieces(pieces: &[&[u8]]) -> FilterResult {
    let mut filter = ScrollbackWriteFilter::new();
    let mut emitted = Vec::new();
    for piece in pieces {
        let (_dims, out) = filter.feed(piece, DIMS);
        emitted.extend_from_slice(&out);
    }
    FilterResult {
        emitted,
        pending: filter.pending().to_vec(),
        awaiting: filter.awaiting_designator(),
    }
}

/// Feed `input` in one call, at every two-way split and one byte at a time;
/// every way must give the one-call result. Returns that result.
fn assert_split_invariant(input: &[u8], what: &str) -> FilterResult {
    let whole = feed_pieces(&[input]);
    for split in 0..=input.len() {
        let got = feed_pieces(&[&input[..split], &input[split..]]);
        assert_eq!(
            got, whole,
            "{what}: split at {split} of {input:?} differs from a single call"
        );
    }
    let bytes: Vec<&[u8]> = input.chunks(1).collect();
    assert_eq!(
        feed_pieces(&bytes),
        whole,
        "{what}: byte-at-a-time feed of {input:?} differs from a single call"
    );
    whole
}

/// A cut-aware feed's emitted bytes and remaining state, for one call that
/// carries `cuts` (fed coordinates of that call).
fn feed_with(filter: &mut ScrollbackWriteFilter, fed: &[u8], cuts: &[usize]) -> Vec<u8> {
    filter.feed_with_cuts(fed, DIMS, cuts).bytes
}

// ── AC-1 (FR2, FR3): the shared strip ────────────────────────────────────

/// Runs whose byte after `ESC (` / `ESC )` is an ESC: nothing in them is a
/// strip target for the client.
const DESIGNATOR_ESC_RUNS: &[&[u8]] = &[
    b"\x1b(\x1b]0777;emterm;markdown;begin;id=x\x07X",
    b"\x1b)\x1b[6n",
    b"\x1b(\x1b]777;emterm;markdown;x\x07",
];

fn osc(body: &[u8], terminator: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b]".to_vec();
    v.extend_from_slice(body);
    v.extend_from_slice(terminator);
    v
}

/// AC-1 (FR2, FR3, TM-1, registry): the ESC right after `ESC (` / `ESC )` is
/// the charset designator and never starts an OSC, a CSI query, an APC or a
/// DCS — in the ring-write strip, the snapshot strip, the remapping strip
/// and, for the ring write, through the write filter in one call and at every
/// split position. A launch that follows a COMPLETED designation is still
/// stripped.
#[test]
fn round3_76342d8d_designator_esc_never_starts_a_strip_target_in_either_strip() {
    for input in DESIGNATOR_ESC_RUNS {
        let label = String::from_utf8_lossy(input);
        assert_eq!(
            strip_pty_output_for_scrollback_write(input),
            *input,
            "{label:?}: ring-write strip keeps every byte"
        );
        assert_eq!(
            strip_replayable_rich_content(input),
            *input,
            "{label:?}: snapshot strip keeps every byte"
        );
        assert_eq!(
            strip_rich_content_and_remap(input, &[]).0,
            *input,
            "{label:?}: remapping strip keeps every byte"
        );

        // Through the write filter, in one call and at every split position.
        let whole = assert_split_invariant(input, "AC-1");
        assert_eq!(
            whole.emitted, *input,
            "{label:?}: the filter keeps every byte"
        );
        assert!(whole.pending.is_empty(), "{label:?}: nothing is held");
    }

    // A launch after a completed designation is a real launch: stripped by
    // both strips, with either brace and either terminator, and through the
    // write filter at every split.
    for designation in [&b"\x1b(B"[..], &b"\x1b)0"[..]] {
        for terminator in [&b"\x07"[..], &b"\x1b\\"[..]] {
            let launch = osc(b"777;emterm;markdown;begin;id=x", terminator);
            let mut input = b"pre".to_vec();
            input.extend_from_slice(designation);
            input.extend_from_slice(&launch);
            input.extend_from_slice(b"X");
            let mut expected = b"pre".to_vec();
            expected.extend_from_slice(designation);
            expected.extend_from_slice(b"X");
            assert_eq!(strip_pty_output_for_scrollback_write(&input), expected);
            assert_eq!(strip_replayable_rich_content(&input), expected);
            let whole = assert_split_invariant(&input, "AC-1 launch after a designation");
            assert_eq!(whole.emitted, expected);
        }
    }

    // The state-taking form with the flag set copies byte 0 verbatim — even
    // an ESC that would open a launch — and resumes after it.
    let launch = osc(b"777;emterm;markdown;begin;id=x", b"\x07");
    assert_eq!(
        strip_pty_output_for_scrollback_write_with_designator(&launch, true),
        launch
    );
    let mut with_plain_designator = b"B".to_vec();
    with_plain_designator.extend_from_slice(&launch);
    with_plain_designator.push(b'Z');
    assert_eq!(
        strip_pty_output_for_scrollback_write_with_designator(&with_plain_designator, true),
        b"BZ"
    );

    // With the flag clear the state-taking form equals the existing entry
    // points on a mixed corpus.
    let mut corpus: Vec<Vec<u8>> = DESIGNATOR_ESC_RUNS.iter().map(|r| r.to_vec()).collect();
    corpus.push([b"a".as_slice(), &launch, b"b"].concat());
    corpus.push(b"x\x1b[6ny\x1b(B\x1b_Gi=1;P\x1b\\z".to_vec());
    for input in &corpus {
        let watch: Vec<usize> = (0..=input.len()).collect();
        assert_eq!(
            strip_pty_output_for_scrollback_write_with_designator(input, false),
            strip_pty_output_for_scrollback_write(input),
            "{input:?}"
        );
        assert_eq!(
            strip_rich_content_and_remap_with_designator(input, &watch, false),
            strip_rich_content_and_remap(input, &watch),
            "{input:?}"
        );
    }

    // Remapped watch offsets before, on and after the designator byte stay
    // consistent with the output.
    let run = DESIGNATOR_ESC_RUNS[0];
    let watch: Vec<usize> = (0..=run.len()).collect();
    let (out, remapped) = strip_rich_content_and_remap(run, &watch);
    assert_eq!(out, run.to_vec());
    assert_eq!(remapped, watch, "every kept byte maps one-to-one");

    // `ab`, a completed designation whose designator is `B`, a launch (which
    // IS stripped), `Z`: offsets before, on and after the designator byte
    // keep their place; offsets inside and after the launch shift by its
    // length.
    let mut input = b"ab\x1b(B".to_vec();
    input.extend_from_slice(&launch);
    input.push(b'Z');
    let launch_start = 5usize;
    let z = launch_start + launch.len();
    let watch = [1usize, 2, 3, 4, launch_start, launch_start + 3, z, z + 1];
    let (out, remapped) = strip_rich_content_and_remap(&input, &watch);
    assert_eq!(out, b"ab\x1b(BZ".to_vec());
    assert_eq!(remapped, vec![1, 2, 3, 4, 5, 5, 5, 6]);

    // The flag set: byte 0 is the designator, kept at offset 0, and its ESC
    // does not open the launch behind it.
    let (out, remapped) =
        strip_rich_content_and_remap_with_designator(&launch, &[0, 1, launch.len()], true);
    assert_eq!(out, launch);
    assert_eq!(remapped, vec![0, 1, launch.len()]);
}

// ── AC-2 (FR2): split invariance of the write filter ─────────────────────

/// AC-2 (FR2, EC-3, registry): across a designator ESC, every two-way split
/// and byte-at-a-time feeding give the same emitted bytes, the same `pending`
/// and the same awaiting flag as one call. The pre-fix filter emits the whole
/// input when a split falls right after `ESC ( ESC` (the second call then
/// sees a plain `]...` / `[6n`) and a stripped run when fed whole.
#[test]
fn round3_195916fd_write_filter_output_is_split_invariant_across_a_designator_esc() {
    let corpus: Vec<&[u8]> = vec![
        b"\x1b(\x1b[6n",
        b"\x1b(\x1b]777;emterm;markdown;x\x07",
        b"\x1b)\x1b[6n",
        b"x\x1b)\x1b]777;emterm;markdown;begin\x07y",
        b"\x1b(\x1b(\x1b]777;emterm;markdown;x\x07",
        b"\x1b(\x1b\x1b]777;emterm;markdown;x\x07",
        b"\x1b(B\x1b]777;emterm;markdown;x\x07z",
        b"\x1b(\x1b_Gi=1,a=T;PAYLOAD\x1b\\tail",
        b"\x1b)\x1bPq#0;2;0;0;0\x1b\\tail",
        b"\x1b(\x1b]0;t\x07\x1b(",
        b"\x1b(\x1b",
    ];
    for input in &corpus {
        assert_split_invariant(input, "AC-2");
    }

    // The same for the kept-byte examples: nothing is emitted differently
    // from the input itself, wherever the split falls.
    for input in &corpus[..2] {
        let whole = feed_pieces(&[*input]);
        assert_eq!(whole.emitted, input.to_vec());
    }

    // A cut placed right after a designator-start segment: feeding the same
    // bytes in two calls (the cut keeps its fed position) emits what one call
    // emits. The cut clears the awaiting flag, so the bytes after it start
    // from ground. A CSI query that follows the cut is stripped only when it
    // arrives within one call (an incomplete CSI is never held), so splits
    // that fall inside it are outside this invariant.
    for (input, inside_csi) in [
        (&b"\x1b(\x1b[6n"[..], 3..=5usize),
        (&b"\x1b(\x1b]777;emterm;markdown;x\x07"[..], 1..=0usize),
    ] {
        let cut = 2usize;
        let mut one = ScrollbackWriteFilter::new();
        let one_out = feed_with(&mut one, input, &[cut]);
        let mut compared = 0usize;
        for split in 0..=input.len() {
            if inside_csi.contains(&split) {
                continue;
            }
            compared += 1;
            let mut f = ScrollbackWriteFilter::new();
            let (head, tail) = input.split_at(split);
            let head_cuts: Vec<usize> = if cut <= split { vec![cut] } else { vec![] };
            let tail_cuts: Vec<usize> = if cut > split {
                vec![cut - split]
            } else {
                vec![]
            };
            let mut got = feed_with(&mut f, head, &head_cuts);
            got.extend_from_slice(&feed_with(&mut f, tail, &tail_cuts));
            assert_eq!(
                got, one_out,
                "cut after the designator start, split at {split} of {input:?}"
            );
            assert_eq!(f.pending(), one.pending(), "split at {split}: pending");
            assert_eq!(
                f.awaiting_designator(),
                one.awaiting_designator(),
                "split at {split}: awaiting flag"
            );
        }
        assert!(compared > 2);
        assert_eq!(
            one_out,
            b"\x1b(".to_vec(),
            "the segment before the cut is kept; what follows starts from ground \
             and is stripped"
        );
    }

    // An overflow flush that starts in the awaiting-designator state passes
    // that state to the strip: the leading ESC is the designator, so the
    // launch behind it is not removed from the flushed bytes.
    let launch = osc(b"777;emterm;markdown;begin", b"\x07");
    for tail in [&b""[..], &b"\x1b("[..], &b"\x1b(\x1b"[..]] {
        let mut f = ScrollbackWriteFilter::new();
        f.feed(b"\x1b(", DIMS);
        assert!(f.awaiting_designator());
        let mut fed = launch.clone();
        fed.extend(std::iter::repeat_n(b'p', SCROLLBACK_FILTER_PENDING_CAP));
        fed.extend_from_slice(tail);
        let outcome = f.feed_with_cuts(&fed, DIMS, &[]);
        assert!(f.pending().is_empty(), "the overflow flush empties pending");
        assert_eq!(
            outcome.bytes, fed,
            "tail {tail:?}: the designator ESC keeps the launch bytes in the flushed run"
        );
        // The flag equals the client-parity state at the end of the flushed
        // run: awaited only when the run ends right after `ESC (`.
        assert_eq!(
            f.awaiting_designator(),
            tail == b"\x1b(",
            "tail {tail:?}: awaiting flag after the flush"
        );
    }

    // The same flush with the flag clear strips the launch (the strip is not
    // bypassed).
    let mut g = ScrollbackWriteFilter::new();
    let mut fed = launch.clone();
    fed.extend(std::iter::repeat_n(b'p', SCROLLBACK_FILTER_PENDING_CAP));
    let outcome = g.feed_with_cuts(&fed, DIMS, &[]);
    assert_eq!(outcome.bytes, vec![b'p'; SCROLLBACK_FILTER_PENDING_CAP]);
}

// ── AC-3 (FR1): constructs closed at a cut are not written ───────────────

/// AC-3 (FR1, TM-1, EC-1, EC-2): at a cut, an OSC, DCS or APC string or a
/// held lone ESC is absent from the emitted bytes from its opening ESC on —
/// opened in the segment or carried in `pending`. The settled bytes before it
/// are emitted through the strip, no terminator is added, `pending` is empty.
#[test]
fn a_cut_drops_the_construct_from_its_opening_esc_on() {
    let launch = osc(b"777;emterm;markdown;begin;id=x", b"\x07");

    // Opened in the segment, for each construct kind.
    for (name, open) in [
        ("OSC", &b"\x1b]0;title"[..]),
        ("DCS", &b"\x1bPq#0;2"[..]),
        ("APC", &b"\x1b_Gi=1;PAY"[..]),
        ("lone ESC", &b"\x1b"[..]),
    ] {
        let mut fed = b"abc".to_vec();
        fed.extend_from_slice(&launch);
        fed.extend_from_slice(b"def");
        fed.extend_from_slice(open);
        let mut f = ScrollbackWriteFilter::new();
        let outcome = f.feed_with_cuts(&fed, DIMS, &[fed.len()]);
        assert_eq!(
            outcome.bytes,
            b"abcdef".to_vec(),
            "{name}: settled bytes go through the strip (the launch is removed), \
             the construct and any terminator are absent"
        );
        assert!(f.pending().is_empty(), "{name}: nothing is held");
        assert!(outcome.carried.is_none(), "{name}");

        // With bytes after the cut, which start from ground.
        let mut g = ScrollbackWriteFilter::new();
        let mut cut_fed = fed.clone();
        cut_fed.extend_from_slice(b"ZZ");
        let outcome = g.feed_with_cuts(&cut_fed, DIMS, &[fed.len()]);
        assert_eq!(
            outcome.bytes,
            b"abcdefZZ".to_vec(),
            "{name}: bytes after the cut"
        );
        assert!(g.pending().is_empty());
    }

    // Carried in `pending` from an earlier call. The continuation keeps the
    // construct incomplete; the cut falls inside it.
    for (name, open, cont) in [
        ("OSC", &b"\x1b]0;ti"[..], &b"mid!"[..]),
        ("DCS", &b"\x1bPq#0"[..], &b"mid!"[..]),
        ("APC", &b"\x1b_Gi=1;PA"[..], &b"mid!"[..]),
        ("lone ESC", &b"\x1b"[..], &b"]mi!"[..]),
    ] {
        let mut f = ScrollbackWriteFilter::new();
        let (_d, first) = f.feed(open, DIMS);
        assert!(first.is_empty(), "{name}: held back");
        assert_eq!(f.pending(), open);
        // The next call closes it with a cut at fed 0 and brings bytes.
        let outcome = f.feed_with_cuts(b"xyz", DIMS, &[0]);
        assert_eq!(
            outcome.bytes,
            b"xyz".to_vec(),
            "{name}: carried construct dropped"
        );
        assert!(f.pending().is_empty());
        assert!(outcome.carried.is_none());

        // A cut after some bytes of the continuation: the carried run and the
        // part of the continuation before the cut are dropped, the byte after
        // the cut starts from ground.
        let mut g = ScrollbackWriteFilter::new();
        g.feed(open, DIMS);
        let outcome = g.feed_with_cuts(cont, DIMS, &[3]);
        assert_eq!(
            outcome.bytes,
            b"!".to_vec(),
            "{name}: the carried run and its continuation up to the cut are dropped"
        );
        assert!(g.pending().is_empty());
        assert!(outcome.carried.is_none());
    }

    // An empty fed range with a cut closes and drops what is held.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b]11;?", DIMS);
    let outcome = f.feed_with_cuts(b"", DIMS, &[0]);
    assert!(outcome.bytes.is_empty());
    assert!(f.pending().is_empty());

    // EC-1: a carried-over sequence that completed before the cut is still
    // reported with the same bytes and fed end, and the construct opened
    // after it is the one that is dropped.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b]0;t", DIMS);
    let fed = b"\x07abc\x1b]1;u";
    let outcome = f.feed_with_cuts(fed, DIMS, &[fed.len()]);
    assert_eq!(
        outcome.bytes,
        b"\x1b]0;t\x07abc".to_vec(),
        "the completed carried OSC is settled and kept; the later open OSC is dropped"
    );
    let carried = outcome
        .carried
        .expect("the completion before the cut is reported");
    assert_eq!(carried.bytes(), b"\x1b]0;t\x07");
    assert_eq!(carried.fed_end(), 1);
    assert!(f.pending().is_empty());

    // The completion ends exactly at the cut.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b]0;t", DIMS);
    let outcome = f.feed_with_cuts(b"\x07", DIMS, &[1]);
    assert_eq!(outcome.bytes, b"\x1b]0;t\x07".to_vec());
    let carried = outcome.carried.expect("reported");
    assert_eq!(carried.bytes(), b"\x1b]0;t\x07");
    assert_eq!(carried.fed_end(), 1);

    // EC-2: an awaiting-designator `ESC (` at a cut is still emitted and the
    // flag is cleared.
    let mut f = ScrollbackWriteFilter::new();
    let outcome = f.feed_with_cuts(b"ab\x1b(", DIMS, &[4]);
    assert_eq!(outcome.bytes, b"ab\x1b(".to_vec());
    assert!(f.pending().is_empty());
    assert!(!f.awaiting_designator());

    // The same when the `ESC (` was fed by an earlier call.
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b(", DIMS);
    assert!(f.awaiting_designator());
    let outcome = f.feed_with_cuts(b"", DIMS, &[0]);
    assert!(outcome.bytes.is_empty());
    assert!(!f.awaiting_designator(), "the cut clears the awaiting flag");
}

/// AC-3: the strip inside the cut path receives the awaiting-designator flag:
/// with a designator carried in, a cut after it still copies the designator
/// ESC verbatim.
#[test]
fn a_cut_after_a_carried_designator_keeps_the_designator_esc() {
    let mut f = ScrollbackWriteFilter::new();
    f.feed(b"\x1b(", DIMS);
    let fed = b"\x1b]777;emterm;markdown;x\x07";
    let outcome = f.feed_with_cuts(fed, DIMS, &[fed.len()]);
    assert_eq!(
        outcome.bytes,
        fed.to_vec(),
        "the first byte is the designator; the rest is plain text"
    );
    assert!(f.pending().is_empty());
}

// ── AC-4 (FR1): a closed OSC is never completed by a later BEL ───────────

/// Answers `OSC 11 ; ?` — but never an `Unterminated` one, as the production
/// color responder — so a test can count color-query answers.
pub(super) struct QueryResponder;

impl term_core::OscResponder for QueryResponder {
    fn respond(
        &self,
        code: u16,
        payload: &str,
        terminator: term_core::OscTerminator,
    ) -> Vec<Vec<u8>> {
        if code == 11 && payload == "?" && terminator != term_core::OscTerminator::Unterminated {
            vec![b"\x1b]11;rgb:1111/2222/3333\x07".to_vec()]
        } else {
            Vec::new()
        }
    }
}

pub(super) fn new_core() -> TerminalCore {
    let mut core = TerminalCore::new(R2_COLS, R2_ROWS, 10_000);
    core.osc_responder = Some(Box::new(QueryResponder));
    core
}

/// The client as the GUI drives it: a snapshot chunk through
/// `reset_and_replay_segments` with its responses discarded, every
/// `PtyOutput` chunk through `process_pty_data_fully`. Returns the client and
/// every response it produced from `PtyOutput` bytes.
pub(super) fn client_view(received: &[PtyOutputChunk]) -> (TerminalCore, Vec<u8>) {
    let mut client = new_core();
    let mut responses = Vec::new();
    for chunk in received {
        match chunk.kind {
            R2ChunkKind::Snapshot => {
                let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&chunk.data);
                let replay: Vec<ReplaySegment> = segments
                    .iter()
                    .map(|s| ReplaySegment {
                        offset: s.offset,
                        cols: s.cols,
                        rows: s.rows,
                    })
                    .collect();
                client.reset_and_replay_segments(content, &replay);
                let _discarded = client.take_response();
            }
            R2ChunkKind::PtyOutput => {
                if !chunk.data.is_empty() {
                    client.process_pty_data_fully(&chunk.data);
                    responses.extend(client.take_response());
                }
            }
        }
    }
    (client, responses)
}

pub(super) fn reference_view(chunks: &[Vec<u8>]) -> (TerminalCore, Vec<u8>) {
    let mut reference = new_core();
    reference.process_pty_data_fully(&chunks.concat());
    let responses = reference.take_response();
    (reference, responses)
}

/// Responses, screen, cursor and displayed characters of the client equal the
/// raw-stream reference's, unconditionally (the FR1 oracle convention).
pub(super) fn assert_client_equals_reference(
    received: &[PtyOutputChunk],
    chunks: &[Vec<u8>],
    ctx: &str,
) {
    let (client, client_responses) = client_view(received);
    let (reference, reference_responses) = reference_view(chunks);
    assert_eq!(
        client_responses, reference_responses,
        "{ctx}: response bytes differ from the raw-stream reference"
    );
    for r in 0..R2_ROWS {
        let got = client.get_line_text(r);
        assert!(
            !got.contains('\u{fffd}'),
            "{ctx}: row {r} displays U+FFFD: {got:?}"
        );
        assert_eq!(
            got.trim_end(),
            reference.get_line_text(r).trim_end(),
            "{ctx}: row {r} differs from the reference"
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

/// Spawn `pty_reader_loop` on a background thread against `pane`, fed
/// `chunks`, WITHOUT touching the pane's output target.
pub(super) fn spawn_reader_keeping_target(
    pane: &MuxPane,
    chunks: Vec<Vec<u8>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn({
        let output_target = pane.output_target.clone();
        let shadow_parser = pane.shadow_parser.clone();
        let cwd = pane.cwd.clone();
        let title = pane.title.clone();
        let title_sender = pane.title_sender.clone();
        let notification_sender = pane.notification_sender.clone();
        let agent_status_report_sender = pane.agent_status_report_sender.clone();
        let raw_passthrough = pane.raw_passthrough.clone();
        let passthrough_scanner = pane.passthrough_scanner.clone();
        let scrollback = pane.scrollback.clone();
        let dims = pane.dims.clone();
        let output_capture = pane.output_capture.clone();
        let pane_id = pane.id;
        move || {
            pty_reader_loop(
                pane_id,
                Box::new(ScriptedReader::new(chunks)),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                output_capture,
            );
        }
    })
}

/// What one scripted reader run delivered, with the ring it left.
pub(super) struct RestoreRun {
    /// Everything the owner's channel received, in order (the snapshot of the
    /// visibility restore, forwarded reads and replacements, EOF last).
    pub(super) received: Vec<PtyOutputChunk>,
    pub(super) ring: Vec<u8>,
}

/// Drive `chunks` through the production reader on a main-screen pane that is
/// hidden by visibility, and run the PRODUCTION visibility restore
/// (`resume_pane_with_permit`) while the reader is paused at P2 of read
/// `restore_read` (after its capture step, before its forward decision), so
/// that read is covered by the restore's snapshot and suppressed. The pane's
/// ring has not wrapped, so the snapshot has no screen-dump block: its
/// content is the ring alone, with no ESC of a dump to abort an open OSC.
pub(super) fn run_visibility_restore_at(chunks: &[Vec<u8>], restore_read: usize) -> RestoreRun {
    assert!(restore_read < chunks.len());
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(tx.clone()),
    }));
    let pane = MuxPane::new_test(400, R2_COLS, R2_ROWS, output_target);
    let output_capture = pane.output_capture.clone();
    let total = chunks.len();

    let (mut arrived_rx, mut release_tx) = output_capture.p2.arm();
    let handle = spawn_reader_keeping_target(&pane, chunks.to_vec());
    for i in 0..total {
        arrived_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the reader must reach P2");
        // Re-arm while the reader is still blocked inside the old hit().
        let next = (i + 1 < total).then(|| output_capture.p2.arm());
        if i == restore_read {
            let permit = tx.try_reserve().expect("capacity for the resume permit");
            let outcome = resume_pane_with_permit(&pane, &tx, AnyPermit::Borrowed(permit), 10_000);
            assert!(matches!(outcome, ResumeOutcome::Resumed));
        }
        release_tx.send(()).unwrap();
        if let Some((a, r)) = next {
            arrived_rx = a;
            release_tx = r;
        }
    }
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received.first().map(|c| c.kind),
        Some(R2ChunkKind::Snapshot),
        "the restore's snapshot is the first delivery"
    );
    assert!(
        received.last().is_some_and(|c| c.data.is_empty()),
        "the last chunk is the EOF marker"
    );
    let ring = pane.scrollback.lock().unwrap().read_all();
    RestoreRun { received, ring }
}

/// Count the occurrences of an OSC introducer in `bytes`.
pub(super) fn osc_introducers(bytes: &[u8]) -> usize {
    r2_count(bytes, b"\x1b]")
}

/// AC-4 (FR1, TM-1, TS-1, registry): a suppressed chunk of `ESC ]11;?`, a
/// removed 47 / 1047 / 1049 `h` / `l` pair and a read holding BEL, through the
/// production visibility restore of a main-screen pane whose ring has not
/// wrapped. The OSC the client's switch ESC closed is not written to the ring,
/// so the later BEL completes nothing: the client produces no response and
/// equals the raw-stream reference. Repeated with a trailing space, with the
/// OSC carried in `pending` from an earlier read, and with a lone ESC before
/// the switch. The pre-fix ring holds the unterminated OSC, which the BEL
/// completes into a color-query answer the raw stream never produced.
#[test]
fn round3_973af79f_osc_closed_by_a_removed_switch_is_never_completed_by_a_later_bel() {
    for (enter, leave) in switch_pairs() {
        let form = String::from_utf8_lossy(enter).into_owned();
        for trailing in [&b""[..], &b" "[..]] {
            let pair = [enter, leave, trailing].concat();

            // (a) the OSC and the switch pair in one suppressed read.
            let chunks = vec![[b"\x1b]11;?".as_slice(), &pair].concat(), b"\x07".to_vec()];
            let ctx = format!("(a) {form} trailing {trailing:?}");
            let run = run_visibility_restore_at(&chunks, 0);
            assert_eq!(
                osc_introducers(&run.ring),
                0,
                "{ctx}: the ring holds no OSC"
            );
            assert_client_equals_reference(&run.received, &chunks, &ctx);

            // (b) the OSC carried in `pending` from an earlier read.
            let chunks = vec![
                b"\x1b]11;".to_vec(),
                [b"?".as_slice(), &pair].concat(),
                b"\x07".to_vec(),
            ];
            let ctx = format!("(b) carried {form} trailing {trailing:?}");
            let run = run_visibility_restore_at(&chunks, 1);
            assert_eq!(
                osc_introducers(&run.ring),
                0,
                "{ctx}: the ring holds no OSC"
            );
            assert_client_equals_reference(&run.received, &chunks, &ctx);

            // (c) a lone ESC before the switch; the following read holds the
            // rest of what would be an OSC.
            let chunks = vec![[b"\x1b".as_slice(), &pair].concat(), b"]11;?\x07".to_vec()];
            let ctx = format!("(c) lone ESC {form} trailing {trailing:?}");
            let run = run_visibility_restore_at(&chunks, 0);
            assert_eq!(
                osc_introducers(&run.ring),
                0,
                "{ctx}: the ring holds no OSC"
            );
            assert_client_equals_reference(&run.received, &chunks, &ctx);
        }
    }
}

// ── AC-5 (FR4): the fallback closes the held construct ───────────────────

/// AC-5 (FR4, TM-1, TS-4, registry): reads `ESC` / `[?1049h` / `ESC` /
/// `[?1049l` / `]11;? BEL` (and the 47 / 1047 shapes), the last read
/// suppressed. The switch straddles reads, so the reader takes the fallback
/// path with the alternate screen involved; it closes the write filter's held
/// lone ESC, so BEL completes nothing: no carried-over completion is
/// reported, the replacement holds no color query and the client's responses
/// equal the raw-stream reference's. The pre-fix reader leaves the ESC held,
/// joins it with the last read into an OSC and reports its completion.
#[test]
fn round3_2a9d929f_straddling_switch_fallback_closes_the_held_construct() {
    for (enter, leave) in switch_pairs() {
        let form = String::from_utf8_lossy(enter).into_owned();
        // The straddle splits each sequence after its ESC.
        let chunks = vec![
            b"\x1b".to_vec(),
            enter[1..].to_vec(),
            b"\x1b".to_vec(),
            leave[1..].to_vec(),
            b"]11;?\x07".to_vec(),
        ];
        let run = run_reader_with_suppressed_reads(&chunks, &[4]);
        let delivered = run.pty_output_bytes();
        assert_eq!(
            r2_count(&delivered, b"\x1b]11;?"),
            0,
            "{form}: the replacement and the forwarded reads hold no color query"
        );
        assert_eq!(
            osc_introducers(&run.ring),
            0,
            "{form}: the ring holds no OSC introducer"
        );
        assert!(
            run.ring.ends_with(b"]11;?\x07"),
            "{form}: the last read reaches the ring as plain text"
        );
        let (_client, responses) = client_view(&run.received);
        let (_reference, reference_responses) = reference_view(&chunks);
        assert_eq!(
            responses, reference_responses,
            "{form}: the client's responses equal the raw-stream reference's"
        );
        assert!(
            responses.is_empty(),
            "{form}: the raw stream answers nothing here"
        );
        let stream = chunks.concat();
        if r2_snapshot_reproduces_prefix(&run, &stream, stream.len()) {
            assert_r2_screen_matches(&run, &chunks, &format!("{form} AC-5"));
        }
    }
}

// ── AC-6 (TM-2, NFR5, TS-9): adversarial inputs ──────────────────────────

const BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

/// AC-6: designator chains through the strip and the filter, in one call and
/// split, finish within the budget and keep every byte.
#[test]
fn designator_chains_finish_within_the_budget_in_the_strip_and_the_filter() {
    for unit in [&b"\x1b("[..], &b"\x1b(\x1b"[..], &b"\x1b)\x1b)"[..]] {
        let input: Vec<u8> = std::iter::repeat_n(unit, 30_000)
            .flatten()
            .copied()
            .collect();
        let start = std::time::Instant::now();
        assert_eq!(strip_pty_output_for_scrollback_write(&input), input);
        assert_eq!(strip_replayable_rich_content(&input), input);
        let whole = feed_pieces(&[&input]);
        assert_eq!(whole.emitted, input, "unit {unit:?}");
        for split in [1usize, 2, 3, 4, 5, input.len() / 2, input.len() - 1] {
            let got = feed_pieces(&[&input[..split], &input[split..]]);
            assert_eq!(got, whole, "unit {unit:?} split at {split}");
        }
        assert!(
            start.elapsed() < BUDGET,
            "chains of {unit:?} took {:?}",
            start.elapsed()
        );
    }
}

/// AC-6: switch sequences straddling many consecutive reads exercise the
/// fallback closing repeatedly. Every chunk the reader sends that is not the
/// EOF marker is non-empty, and the ring holds only the settled bytes.
#[test]
fn straddling_switches_over_many_reads_close_repeatedly_and_never_send_an_empty_chunk() {
    // Through the suppressed pipeline: a few rounds fit the channel.
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..2 {
        chunks.push(b"x\x1b]0;t".to_vec());
        chunks.push(b"\x1b".to_vec());
        chunks.push(b"[?1049h".to_vec());
        chunks.push(b"\x1b".to_vec());
        chunks.push(b"[?1049l".to_vec());
    }
    chunks.push(b"\x07z".to_vec());
    let last = chunks.len() - 1;
    let start = std::time::Instant::now();
    let run = run_reader_with_suppressed_reads(&chunks, &[last]);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    let (eof, rest) = run.received.split_last().expect("deliveries");
    assert!(eof.data.is_empty(), "the last chunk is the EOF marker");
    for chunk in rest {
        assert!(
            !chunk.data.is_empty(),
            "every chunk other than the EOF marker is non-empty"
        );
    }
    assert_eq!(
        run.ring,
        b"xx\x07z".to_vec(),
        "each round's open OSC is closed by the fallback and dropped"
    );
    let (_client, responses) = client_view(&run.received);
    let (_reference, reference_responses) = reference_view(&chunks);
    assert_eq!(responses, reference_responses);

    // Many rounds with nothing forwarded: a pane with no connected owner.
    let rounds = 400usize;
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..rounds {
        chunks.push(b"x\x1b]0;t".to_vec());
        chunks.push(b"\x1b".to_vec());
        chunks.push(b"[?1049h".to_vec());
        chunks.push(b"\x1b".to_vec());
        chunks.push(b"[?1049l".to_vec());
    }
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(chunks);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_eq!(ring, vec![b'x'; rounds]);
}

/// Run `chunks` through the production reader on a pane with no connected
/// owner (nothing is forwarded); returns the ring.
pub(super) fn run_reader_without_owner(chunks: Vec<Vec<u8>>) -> Vec<u8> {
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(401, R2_COLS, R2_ROWS, output_target);
    spawn_reader_keeping_target(&pane, chunks).join().unwrap();
    pane.scrollback.lock().unwrap().read_all()
}

/// AC-6: an OSC held near the 512 KiB cap that is closed at a cut — and by the
/// reader's fallback — is dropped; the cap and the strip-filtered overflow
/// flush are unchanged.
#[test]
fn an_osc_held_near_the_cap_is_dropped_at_a_cut_and_at_the_fallback() {
    // Filter level: pending exactly at the cap is still held, and a cut at
    // fed 0 with an empty range closes and drops it.
    let intro = b"\x1b]0;".to_vec();
    let mut f = ScrollbackWriteFilter::new();
    f.feed(&intro, DIMS);
    let pad = SCROLLBACK_FILTER_PENDING_CAP - intro.len();
    let (_d, out) = f.feed(&vec![b'p'; pad], DIMS);
    assert!(out.is_empty());
    assert_eq!(f.pending_len(), SCROLLBACK_FILTER_PENDING_CAP);
    let start = std::time::Instant::now();
    let outcome = f.feed_with_cuts(b"", DIMS, &[0]);
    assert!(start.elapsed() < BUDGET);
    assert!(
        outcome.bytes.is_empty(),
        "the held OSC is dropped, not written"
    );
    assert!(f.pending().is_empty());

    // A cut at the end of a continuation that keeps the run at the cap.
    let mut g = ScrollbackWriteFilter::new();
    g.feed(&intro, DIMS);
    g.feed(&vec![b'p'; pad - 10], DIMS);
    let outcome = g.feed_with_cuts(&[b'p'; 10], DIMS, &[10]);
    assert!(outcome.bytes.is_empty());
    assert!(g.pending().is_empty());

    // One byte past the cap is the unchanged strip-filtered overflow flush.
    let mut h = ScrollbackWriteFilter::new();
    h.feed(&intro, DIMS);
    h.feed(&vec![b'p'; pad - 10], DIMS);
    let outcome = h.feed_with_cuts(&[b'p'; 11], DIMS, &[]);
    assert_eq!(
        outcome.bytes.len(),
        SCROLLBACK_FILTER_PENDING_CAP + 1,
        "the overflow flush still emits the run"
    );
    assert!(h.pending().is_empty());

    // Reader level: the OSC grows over several reads (each within the read
    // buffer) to just under the cap, then a straddling switch closes it.
    let mut chunks: Vec<Vec<u8>> = vec![b"\x1b]0;".to_vec()];
    chunks.extend(std::iter::repeat_n(vec![b'p'; 60_000], 8));
    chunks.push(b"\x1b".to_vec());
    chunks.push(b"[?1049h".to_vec());
    chunks.push(b"\x1b".to_vec());
    chunks.push(b"[?1049l".to_vec());
    chunks.push(b"tail".to_vec());
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(chunks);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_eq!(
        ring,
        b"tail".to_vec(),
        "the near-cap OSC is closed by the fallback and never written"
    );
}
