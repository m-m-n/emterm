//! mux-suppressed-output-round4-fixes task0005 (FR5): a screen-switch
//! sequence (47 / 1047 / 1049 `h` / `l`) whose `ESC` sits in the charset
//! designator slot right after `ESC (` / `ESC )`.
//!
//! `term_core` consumes that `ESC` as the designator and prints the rest of
//! the sequence as text. Main-span extraction used to remove the sequence as a
//! screen switch, which also fed the cut derivation and the live spans.
//!
//! Oracle convention (IMPLEMENTATION.md "Reader-level oracle convention"): the
//! reference is `term_core` fed the concatenated raw chunks once; the client
//! applies the production visibility-restore snapshot, the replacement of the
//! covered read and every later chunk. Responses, screen, cursor and displayed
//! characters are compared.
//!
//! What the oracle cannot cover (D4): the vt100 shadow parser treats `ESC` as
//! an abort, so it enters the alternate screen on a designator-slot `h` form
//! that `term_core` prints as text, and the snapshot assembly lays the pane
//! out as an alternate-screen pane from then on. A snapshot taken while the
//! shadow is in that state cannot equal the reference, so for an `h` form that
//! nothing closes the cases here compare the RING (replayed through
//! `term_core`) with the reference instead of a snapshot.

use super::round3_write_path::{
    assert_client_equals_reference, new_core, reference_view, run_reader_without_owner,
    run_visibility_restore_at, switch_pairs,
};
use super::*;
use std::borrow::Cow;
use std::ops::Range;
use term_core::terminal_core::TerminalCore;

/// The two designation introducers, each as `(ESC, brace)`.
fn braces() -> [(u8, u8); 2] {
    [(0x1b, b'('), (0x1b, b')')]
}

fn esc_brace(brace: (u8, u8)) -> Vec<u8> {
    vec![brace.0, brace.1]
}

/// `stream` with its first occurrence of `needle` removed.
fn without_first(stream: &[u8], needle: &[u8]) -> Vec<u8> {
    let at = stream
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("the needle occurs in the stream");
    [&stream[..at], &stream[at + needle.len()..]].concat()
}

/// One layout of a stream across reads. `ring_without` names the switch
/// sequence the extraction legitimately removes (a switch outside the
/// designator slot), `None` when every byte belongs in the ring. The
/// visibility restore runs at each read from `first_restore` up to (not
/// including) `end_restore`, `None` meaning the last read: a restore before
/// `first_restore` would find the shadow parser in the alternate screen (see
/// the module doc), and one at or after `end_restore` would leave the effect
/// of a read the pane held back (a color query's response) out of what the
/// client ever receives.
struct Layout {
    name: &'static str,
    chunks: Vec<Vec<u8>>,
    ring_without: Option<Vec<u8>>,
    first_restore: usize,
    end_restore: Option<usize>,
}

fn layouts(brace: (u8, u8), enter: &[u8], leave: &[u8]) -> Vec<Layout> {
    let eb = esc_brace(brace);
    let pre_eb = [b"pre".as_slice(), &eb].concat();
    let paren = [brace.1];
    let query = b"\x1b]11;?\x07";
    let mut out = Vec::new();

    // The `l` form alone in the slot: nothing leaves the main screen, so the
    // sequence is text and every byte belongs in the ring.
    out.push(Layout {
        name: "leave, one read",
        chunks: vec![
            [pre_eb.as_slice(), leave, b"TEXT"].concat(),
            b"later".to_vec(),
        ],
        ring_without: None,
        first_restore: 0,
        end_restore: None,
    });
    out.push(Layout {
        name: "leave, ESC ( ends the previous read",
        chunks: vec![
            pre_eb.clone(),
            [leave, b"TEXT".as_slice()].concat(),
            b"later".to_vec(),
        ],
        ring_without: None,
        first_restore: 0,
        end_restore: None,
    });
    out.push(Layout {
        name: "leave, ESC ends the previous read and the brace opens the next",
        chunks: vec![
            b"pre\x1b".to_vec(),
            [paren.as_slice(), leave, b"TEXT"].concat(),
            b"later".to_vec(),
        ],
        ring_without: None,
        first_restore: 0,
        end_restore: None,
    });
    out.push(Layout {
        name: "leave then a color query, one read",
        chunks: vec![
            [pre_eb.as_slice(), leave, b" ", query, b"post"].concat(),
            b"later".to_vec(),
        ],
        ring_without: None,
        first_restore: 0,
        end_restore: Some(1),
    });

    // The `h` form in the slot followed by a switch that IS one: the shadow
    // parser is back on the main screen at the end of the read.
    out.push(Layout {
        name: "enter then leave, one read",
        chunks: vec![
            [pre_eb.as_slice(), enter, b"mid", leave, b"post"].concat(),
            b"later".to_vec(),
        ],
        ring_without: Some(leave.to_vec()),
        first_restore: 0,
        end_restore: None,
    });
    out.push(Layout {
        name: "enter then leave, ESC ( ends the previous read",
        chunks: vec![
            pre_eb.clone(),
            [enter, b"mid", leave, b"post"].concat(),
            b"later".to_vec(),
        ],
        ring_without: Some(leave.to_vec()),
        first_restore: 0,
        end_restore: None,
    });
    out.push(Layout {
        name: "enter then leave, ESC ends the previous read and the brace opens the next",
        chunks: vec![
            b"pre\x1b".to_vec(),
            [paren.as_slice(), enter, b"mid", leave, b"post"].concat(),
            b"later".to_vec(),
        ],
        ring_without: Some(leave.to_vec()),
        first_restore: 0,
        end_restore: None,
    });
    out.push(Layout {
        name: "enter in one read, leave in the next",
        chunks: vec![
            [pre_eb.as_slice(), enter, b"mid"].concat(),
            [leave, b"post"].concat(),
            b"later".to_vec(),
        ],
        ring_without: Some(leave.to_vec()),
        first_restore: 1,
        end_restore: None,
    });
    out
}

/// Replay `ring` through `term_core` and compare it with the raw-stream
/// reference: responses, rows, cursor.
fn assert_ring_replay_equals_reference(ring: &[u8], chunks: &[Vec<u8>], ctx: &str) {
    let mut replayed = new_core();
    replayed.process_pty_data_fully(ring);
    let replayed_responses = replayed.take_response();
    let (reference, reference_responses) = reference_view(chunks);
    assert_eq!(
        replayed_responses, reference_responses,
        "{ctx}: responses differ from the raw-stream reference"
    );
    for row in 0..24 {
        let got = replayed.get_line_text(row);
        assert!(
            !got.contains('\u{fffd}'),
            "{ctx}: row {row} displays U+FFFD: {got:?}"
        );
        assert_eq!(
            got.trim_end(),
            reference.get_line_text(row).trim_end(),
            "{ctx}: row {row} of the ring replay differs from the reference"
        );
    }
    assert_eq!(
        replayed.get_cursor_row(),
        reference.get_cursor_row(),
        "{ctx}: cursor row of the ring replay"
    );
    assert_eq!(
        replayed.get_cursor_col(),
        reference.get_cursor_col(),
        "{ctx}: cursor col of the ring replay"
    );
}

/// FR5 (SPEC AC-6, TS-9, EC-5, registry): `ESC (` / `ESC )` directly followed
/// by `ESC[?47h` / `ESC[?1047h` / `ESC[?1049h` or the `l` forms, within one
/// read, with `ESC (` ending the previous read and with the `ESC` ending it
/// while the brace opens the next, each followed by text and later chunks.
/// Through the production visibility restore (snapshot, replacement of the
/// covered read, the following reads) the client equals the raw-stream
/// reference: responses, screen, cursor and displayed characters. The ring
/// holds every byte of the designator-slot sequence; only a switch outside
/// the slot is removed.
///
/// An `h` form in the slot that nothing closes leaves the shadow parser in the
/// alternate screen, which the snapshot assembly lays out as such (module
/// doc); for those the ring itself, replayed through `term_core`, is compared.
#[test]
fn round4_fr5_switch_in_the_designator_slot_matches_the_raw_stream_reference() {
    for brace in braces() {
        for (enter, leave) in switch_pairs() {
            let form = String::from_utf8_lossy(enter).into_owned();
            let brace_label = String::from_utf8_lossy(&esc_brace(brace)).into_owned();

            for layout in layouts(brace, enter, leave) {
                let stream = layout.chunks.concat();
                let expected_ring = match &layout.ring_without {
                    Some(removed) => without_first(&stream, removed),
                    None => stream.clone(),
                };
                let end = layout.end_restore.unwrap_or(layout.chunks.len());
                for restore_read in layout.first_restore..end {
                    let ctx = format!(
                        "{} / {brace_label:?} / {form} / restore at read {restore_read}",
                        layout.name
                    );
                    let run = run_visibility_restore_at(&layout.chunks, restore_read);
                    assert_eq!(
                        run.ring, expected_ring,
                        "{ctx}: the ring holds the designator-slot sequence as text"
                    );
                    assert_client_equals_reference(&run.received, &layout.chunks, &ctx);
                }
            }

            // An `h` form in the slot that nothing closes.
            let eb = esc_brace(brace);
            let pre_eb = [b"pre".as_slice(), &eb].concat();
            let held_open: Vec<(&str, Vec<Vec<u8>>)> = vec![
                (
                    "enter, one read",
                    vec![[pre_eb.as_slice(), enter, b"TEXT"].concat()],
                ),
                (
                    "enter, ESC ( ends the previous read",
                    vec![pre_eb.clone(), [enter, b"TEXT".as_slice()].concat()],
                ),
                (
                    "enter, ESC ends the previous read and the brace opens the next",
                    vec![
                        b"pre\x1b".to_vec(),
                        [[brace.1].as_slice(), enter, b"TEXT"].concat(),
                    ],
                ),
            ];
            for (name, chunks) in held_open {
                let ctx = format!("{name} / {brace_label:?} / {form}");
                let ring = run_reader_without_owner(chunks.clone());
                assert_eq!(
                    ring,
                    chunks.concat(),
                    "{ctx}: the ring holds the whole read, the switch text included"
                );
                assert_ring_replay_equals_reference(&ring, &chunks, &ctx);
            }
        }
    }
}

// ── Extraction-level tests ───────────────────────────────────────────────

/// The six recognized switch sequences with the direction each takes. An
/// independent copy: the tests do not read the production table.
const PATTERNS: [(&[u8], bool); 6] = [
    (b"\x1b[?1049h", true),
    (b"\x1b[?1049l", false),
    (b"\x1b[?1047h", true),
    (b"\x1b[?1047l", false),
    (b"\x1b[?47h", true),
    (b"\x1b[?47l", false),
];

/// Small deterministic generator (xorshift), so a failing stream reproduces.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Fragments that, joined at random, form every shape the designator slot
/// cares about: lone and doubled ESC, both braces with and without a
/// designator, the switch sequences whole and in pieces, string introducers
/// and terminators, and noise.
const FRAGMENTS: &[&[u8]] = &[
    b"\x1b",
    b"\x1b",
    b"(",
    b")",
    b"\x1b(",
    b"\x1b)",
    b"\x1b(\x1b",
    b"\x1b)\x1b",
    b"\x1b(B",
    b"\x1b)0",
    b"[",
    b"?",
    b"B",
    b"x",
    b"\\",
    b"\x07",
    b"h",
    b"l",
    b"\x1b]",
    b"\x1bP",
    b"\x1b_",
    b"\x1b[?47",
    b"\x1b[?1049",
    b"\x1b[?1047",
    b"\x1b[?1049h",
    b"\x1b[?1049l",
    b"\x1b[?47h",
    b"\x1b[?47l",
    b"\x1b[?1047h",
    b"\x1b[?1047l",
];

/// The same fragments without a bare brace: a brace only ever appears inside
/// a complete designation, so no `ESC` of the stream is in a designator slot.
const FRAGMENTS_WITHOUT_SLOT: &[&[u8]] = &[
    b"\x1b",
    b"\x1b",
    b"\x1b(B",
    b"\x1b)0",
    b"[",
    b"?",
    b"B",
    b"x",
    b"\\",
    b"\x07",
    b"h",
    b"l",
    b"\x1b]",
    b"\x1bP",
    b"\x1b_",
    b"\x1b[?47",
    b"\x1b[?1049",
    b"\x1b[?1047",
    b"\x1b[?1049h",
    b"\x1b[?1049l",
    b"\x1b[?47h",
    b"\x1b[?47l",
    b"\x1b[?1047h",
    b"\x1b[?1047l",
];

fn random_stream(rng: &mut Rng, fragments: &[&[u8]], max_fragments: usize) -> Vec<u8> {
    let count = 1 + rng.below(max_fragments);
    let mut stream = Vec::new();
    for _ in 0..count {
        stream.extend_from_slice(fragments[rng.below(fragments.len())]);
    }
    stream
}

/// Every occurrence of a switch pattern in `stream`: `(start, pattern, enters)`.
/// A parser that treats `ESC` as an abort acts on each of them.
fn pattern_occurrences(stream: &[u8]) -> Vec<(usize, &'static [u8], bool)> {
    let mut found = Vec::new();
    for at in 0..stream.len() {
        if stream[at] != 0x1b {
            continue;
        }
        if let Some((pat, enters)) = PATTERNS.iter().find(|(p, _)| stream[at..].starts_with(p)) {
            found.push((at, *pat, *enters));
        }
    }
    found
}

/// The switches `term_core` itself dispatches in `stream`, restricted to the
/// six recognized patterns: `(start, pattern, enters)`. `process_pty_data`
/// returns right after the byte that dispatches a buffer switch, and the
/// mode action says which way (term_core's own tests pin the codes: 1 and 2
/// enter the alternate screen, 3 returns to main).
fn term_core_switches(stream: &[u8]) -> Vec<(usize, &'static [u8], bool)> {
    let mut core = TerminalCore::new(80, 24, 100);
    let mut found = Vec::new();
    let mut at = 0usize;
    while at < stream.len() {
        let consumed = core.process_pty_data(&stream[at..]);
        assert!(consumed > 0, "the parser makes progress");
        at += consumed;
        let actions = core.take_mode_actions();
        if let Some(action) = actions.iter().find(|a| matches!(a, 1..=3)) {
            let enters = *action != 3;
            if let Some((pat, pat_enters)) = PATTERNS
                .iter()
                .find(|(p, _)| at >= p.len() && stream[at - p.len()..at] == **p)
            {
                assert_eq!(
                    enters, *pat_enters,
                    "action matches the pattern's direction"
                );
                found.push((at - pat.len(), *pat, enters));
            }
        }
    }
    found
}

/// What extraction must return for the chunk `a..b` of a stream whose
/// term_core switches are `switches`, starting in `alt_at_start`: the
/// main-buffer spans (chunk coordinates), the bytes and the final alt state.
fn expected_extraction(
    stream: &[u8],
    a: usize,
    b: usize,
    switches: &[(usize, &'static [u8], bool)],
    alt_at_start: bool,
) -> (Vec<Range<usize>>, Vec<u8>, bool) {
    let mut spans = Vec::new();
    let mut alt = alt_at_start;
    let mut open: Option<usize> = (!alt).then_some(0);
    for (start, pat, enters) in switches {
        if *start < a || *start >= b {
            continue;
        }
        if let Some(from) = open.take() {
            spans.push(from..*start - a);
        }
        alt = *enters;
        if !alt {
            open = Some(*start - a + pat.len());
        }
    }
    if let Some(from) = open {
        spans.push(from..b - a);
    }
    let bytes = spans
        .iter()
        .flat_map(|r| stream[a + r.start..a + r.end].iter().copied())
        .collect();
    (spans, bytes, alt)
}

/// The split points at which no switch pattern is cut in two (extraction
/// never claims to see one that straddles reads: the reader's cross-check
/// with the shadow parser covers that).
fn allowed_boundaries(stream: &[u8]) -> Vec<usize> {
    let occurrences = pattern_occurrences(stream);
    (1..stream.len())
        .filter(|at| {
            !occurrences
                .iter()
                .any(|(start, pat, _)| *start < *at && *at < start + pat.len())
        })
        .collect()
}

/// Fragments for the comparison with the real vt100 shadow parser: no bare
/// brace, `?`, `[`, `\` or BEL. With them vt100 acts on forms extraction does
/// not recognize and `term_core` does not dispatch (a `)` between the digits
/// and the `h`, a BEL inside the sequence) — an existing, separate difference
/// the reader's cross-check covers.
const VT100_FRAGMENTS: &[&[u8]] = &[
    b"\x1b",
    b"\x1b",
    b"\x1b(",
    b"\x1b)",
    b"\x1b(\x1b",
    b"\x1b)\x1b",
    b"\x1b(B",
    b"\x1b)0",
    b"B",
    b"x",
    b"h",
    b"l",
    b"\x1b]",
    b"\x1bP",
    b"\x1b_",
    b"\x1b[?47",
    b"\x1b[?1049",
    b"\x1b[?47h",
    b"\x1b[?47l",
    b"\x1b[?1049h",
    b"\x1b[?1049l",
];

/// Check extraction of `stream` against `term_core`, in one chunk, at every
/// single split and in every allowed piece with the slot state carried.
/// With `vt100` the chunks also go through the real shadow parser, and the
/// extraction starts each chunk from ITS alternate-screen state as the reader
/// does; `stream` then uses only the forms vt100 acts on.
fn check_extraction_against_term_core(stream: &[u8], vt100: bool, label: &str) -> bool {
    let switches = term_core_switches(stream);
    let occurrences = pattern_occurrences(stream);
    let allowed = allowed_boundaries(stream);
    let mut chunkings: Vec<Vec<usize>> = vec![Vec::new(), allowed.clone()];
    for at in &allowed {
        chunkings.push(vec![*at]);
    }
    for cuts in chunkings {
        let mut bounds = vec![0usize];
        bounds.extend(cuts.iter().copied());
        bounds.push(stream.len());
        let mut shadow = vt100::Parser::new(24, 80, 0);
        let mut shadow_model = false;
        let mut slot = DesignatorSlot::Ground;
        for pair in bounds.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let chunk = &stream[a..b];
            let alt_before = if vt100 {
                shadow.screen().alternate_screen()
            } else {
                shadow_model
            };
            let (spans, bytes, final_alt) =
                expected_extraction(stream, a, b, &switches, alt_before);
            let got = extract_main_buffer(chunk, alt_before, slot);
            let ctx = format!(
                "{label}: stream {:?}, chunk {a}..{b}, cuts {cuts:?}, alt_before {alt_before}, slot {slot:?}",
                String::from_utf8_lossy(stream)
            );
            assert_eq!(got.spans, spans, "{ctx}: spans");
            assert_eq!(got.bytes.as_ref(), bytes.as_slice(), "{ctx}: bytes");
            assert_eq!(got.final_alt, final_alt, "{ctx}: final_alt");

            // The ESC-aborts model: every occurrence in the chunk counts.
            let mut model = alt_before;
            for (start, _pat, enters) in &occurrences {
                if *start >= a && *start < b {
                    model = *enters;
                }
            }
            assert_eq!(got.shadow_alt, model, "{ctx}: shadow_alt");
            if vt100 {
                shadow.process(chunk);
                assert_eq!(
                    got.shadow_alt,
                    shadow.screen().alternate_screen(),
                    "{ctx}: shadow_alt equals the vt100 shadow parser's state"
                );
            } else {
                shadow_model = got.shadow_alt;
            }
            slot = got.slot;
        }
    }
    occurrences.len() != switches.len()
}

/// FR5 (TM-1, AC-2): across random streams, main-span extraction removes
/// exactly the switches `term_core` dispatches — a switch whose `ESC` is a
/// designator is text — wherever the read boundaries fall (the designator-slot
/// state is carried), and its `shadow_alt` is the state an `ESC`-aborts parser
/// reaches. For streams limited to the forms vt100 acts on, `shadow_alt` also
/// equals the real shadow parser's state, which is why the reader compares
/// that value with the shadow.
#[test]
fn extraction_removes_exactly_the_switches_term_core_dispatches() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut with_slot_switch = 0usize;
    for round in 0..6_000 {
        let stream = random_stream(&mut rng, FRAGMENTS, 12);
        let label = format!("model round {round}");
        if check_extraction_against_term_core(&stream, false, &label) {
            with_slot_switch += 1;
        }
    }
    assert!(
        with_slot_switch > 200,
        "the corpus holds streams with a designator-slot switch ({with_slot_switch})"
    );

    let mut with_slot_switch = 0usize;
    for round in 0..4_000 {
        let stream = random_stream(&mut rng, VT100_FRAGMENTS, 12);
        let label = format!("vt100 round {round}");
        if check_extraction_against_term_core(&stream, true, &label) {
            with_slot_switch += 1;
        }
    }
    assert!(
        with_slot_switch > 100,
        "the vt100 corpus holds streams with a designator-slot switch ({with_slot_switch})"
    );
}

// ── AC-3: switches outside a designator slot keep their handling ─────────

/// The extraction algorithm as it was before FR5, kept here as the oracle for
/// everything FR5 must leave alone: every switch pattern is a switch,
/// wherever its `ESC` sits.
fn legacy_extract(data: &[u8], alt_at_start: bool) -> (Cow<'_, [u8]>, bool, Vec<Range<usize>>) {
    let matches_toggle = |d: &[u8]| PATTERNS.iter().find(|(p, _)| d.starts_with(p)).copied();
    let mut has_toggle = false;
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b && matches_toggle(&data[i..]).is_some() {
            has_toggle = true;
            break;
        }
        i += 1;
    }
    if !has_toggle {
        return if alt_at_start {
            (Cow::Borrowed(&[]), true, Vec::new())
        } else {
            (Cow::Borrowed(data), false, vec![0..data.len()])
        };
    }
    let mut out = Vec::with_capacity(data.len());
    let mut spans = Vec::new();
    let mut alt = alt_at_start;
    let mut span_start: Option<usize> = if alt { None } else { Some(0) };
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b {
            if let Some((pat, is_enter)) = matches_toggle(&data[i..]) {
                if let Some(s) = span_start.take() {
                    out.extend_from_slice(&data[s..i]);
                    spans.push(s..i);
                }
                alt = is_enter;
                i += pat.len();
                if !alt {
                    span_start = Some(i);
                }
                continue;
            }
        }
        i += 1;
    }
    if let Some(s) = span_start {
        out.extend_from_slice(&data[s..]);
        spans.push(s..data.len());
    }
    (Cow::Owned(out), alt, spans)
}

/// AC-3 (FR5, NFR1): a stream in which no `ESC` sits in a designator slot —
/// a brace only inside a complete designation such as `ESC ( B` — is
/// extracted exactly as before (bytes, final state, spans), read by read, with
/// the slot state carried and read boundaries anywhere, a switch sequence cut
/// in two included. The cuts and the live spans derived from those spans are
/// therefore unchanged too.
#[test]
fn switches_outside_a_designator_slot_extract_as_before() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for round in 0..4_000 {
        let stream = random_stream(&mut rng, FRAGMENTS_WITHOUT_SLOT, 12);
        for alt0 in [false, true] {
            // Whole, and cut at every position (a cut inside a pattern is the
            // straddling case: neither side sees a switch, before or after).
            let mut cut_sets: Vec<Vec<usize>> = vec![Vec::new()];
            for at in 1..stream.len() {
                cut_sets.push(vec![at]);
            }
            cut_sets.push((1..stream.len()).collect());
            for cuts in cut_sets {
                let mut bounds = vec![0usize];
                bounds.extend(cuts.iter().copied());
                bounds.push(stream.len());
                let mut slot = DesignatorSlot::Ground;
                let mut alt = alt0;
                for pair in bounds.windows(2) {
                    let chunk = &stream[pair[0]..pair[1]];
                    let before = legacy_extract(chunk, alt);
                    let got = extract_main_buffer(chunk, alt, slot);
                    let ctx = format!(
                        "round {round}: stream {:?}, chunk {}..{}, alt {alt}",
                        String::from_utf8_lossy(&stream),
                        pair[0],
                        pair[1]
                    );
                    assert_eq!(got.bytes.as_ref(), before.0.as_ref(), "{ctx}: bytes");
                    assert_eq!(got.final_alt, before.1, "{ctx}: final alt");
                    assert_eq!(got.spans, before.2, "{ctx}: spans");
                    assert_eq!(got.shadow_alt, before.1, "{ctx}: shadow alt");
                    assert_eq!(
                        cuts_from_main_spans(&got.spans, chunk.len()),
                        cuts_from_main_spans(&before.2, chunk.len()),
                        "{ctx}: cuts"
                    );
                    assert_eq!(
                        matches!(got.bytes, Cow::Borrowed(_)),
                        matches!(before.0, Cow::Borrowed(_)),
                        "{ctx}: borrowed exactly when the chunk had no switch"
                    );
                    alt = got.final_alt;
                    slot = got.slot;
                }
            }
        }
    }
}

fn position_of(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("the needle occurs")
}

/// AC-3 / AC-2 (FR5): named cases. A switch after a complete designation is
/// removed, and a switch in the designator slot is kept, in one chunk and
/// across the read boundary; the cuts and live spans follow the corrected
/// span list. The pre-change algorithm removes the slot switch, which is the
/// difference FR5 fixes.
#[test]
fn cuts_and_live_spans_follow_the_corrected_span_list() {
    // After a complete designation: an ordinary switch pair, removed. Cut at
    // the end of the span before the first one.
    let chunk = b"a\x1b(B\x1b[?47hb\x1b[?47lc";
    let got = extract_main_buffer(chunk, false, DesignatorSlot::Ground);
    assert_eq!(got.spans, vec![0..4, 17..18]);
    assert_eq!(got.bytes.as_ref(), b"a\x1b(Bc");
    assert_eq!(cuts_from_main_spans(&got.spans, chunk.len()), vec![4]);
    assert_eq!(got.slot, DesignatorSlot::Ground);

    // The same switch pair with the first one in the designator slot: the `h`
    // stays as text, the `l` is still a switch.
    let chunk = b"a\x1b(\x1b[?47hb\x1b[?47lc";
    let got = extract_main_buffer(chunk, false, DesignatorSlot::Ground);
    assert_eq!(got.spans, vec![0..10, 16..17]);
    assert_eq!(got.bytes.as_ref(), b"a\x1b(\x1b[?47hbc");
    assert!(!got.final_alt);
    assert!(
        !got.shadow_alt,
        "the shadow's `h` and `l` cancel out at the end of the read"
    );
    assert_eq!(cuts_from_main_spans(&got.spans, chunk.len()), vec![10]);
    // The pre-change algorithm cuts at the slot switch instead and drops the
    // text between the two switches.
    let before = legacy_extract(chunk, false);
    assert_eq!(before.2, vec![0..3, 16..17]);
    assert_eq!(cuts_from_main_spans(&before.2, chunk.len()), vec![3]);

    // Only the slot switch: the chunk has no switch to remove, so it is
    // returned borrowed and whole. The shadow parser does switch.
    let chunk = b"a\x1b(\x1b[?47hb";
    let got = extract_main_buffer(chunk, false, DesignatorSlot::Ground);
    assert!(matches!(got.bytes, Cow::Borrowed(_)));
    assert_eq!(got.bytes.as_ref(), chunk);
    assert_eq!(got.spans, vec![0..chunk.len()]);
    assert!(cuts_from_main_spans(&got.spans, chunk.len()).is_empty());
    assert!(!got.final_alt);
    assert!(got.shadow_alt);

    // The `ESC (` ended the previous read: the first byte is the designator.
    let chunk = b"\x1b[?1049hx \x1b[?1049ly";
    let at_l = position_of(chunk, b"\x1b[?1049l");
    let got = extract_main_buffer(chunk, false, DesignatorSlot::Awaiting);
    assert_eq!(got.spans, vec![0..at_l, at_l + 8..chunk.len()]);
    assert_eq!(
        cuts_from_main_spans(&got.spans, chunk.len()),
        vec![at_l],
        "one cut, at the real switch"
    );
    let ground = extract_main_buffer(chunk, false, DesignatorSlot::Ground);
    assert_eq!(ground.spans, vec![0..0, at_l + 8..chunk.len()]);
    assert_eq!(
        cuts_from_main_spans(&ground.spans, chunk.len()),
        vec![0],
        "without the carried state the same bytes are a switch at 0"
    );

    // The `ESC` ended the previous read and the brace opens this one.
    let chunk = b"(\x1b[?47lz";
    let got = extract_main_buffer(chunk, false, DesignatorSlot::EscapeSeen);
    assert!(matches!(got.bytes, Cow::Borrowed(_)));
    assert_eq!(got.spans, vec![0..chunk.len()]);
    let ground = extract_main_buffer(chunk, false, DesignatorSlot::Ground);
    assert_eq!(ground.spans, vec![0..1, 7..8]);

    // A switch straddling two reads is seen by neither (the reader's
    // cross-check with the shadow parser covers it), carried slot or not.
    for slot in [
        DesignatorSlot::Ground,
        DesignatorSlot::EscapeSeen,
        DesignatorSlot::Awaiting,
    ] {
        let first = extract_main_buffer(b"x\x1b[?10", false, slot);
        assert_eq!(first.spans, vec![0..6]);
        let second = extract_main_buffer(b"49hy", false, DesignatorSlot::Ground);
        assert_eq!(second.spans, vec![0..4]);
    }
}

/// FR5 (SPEC FR5, live spans): the spans extraction returns are the live
/// spans the agent-status feed scanner gates OSC 133 marks with. A mark that
/// follows a designator-slot `h` form is live for `term_core` (it stayed on
/// the main screen); the scanner, given the corrected spans, reports it.
#[test]
fn a_mark_after_a_designator_slot_switch_is_live() {
    use crate::mux::session::pane::AgentStatusFeedItem;
    for brace in [&b"\x1b("[..], &b"\x1b)"[..]] {
        for (enter, _leave) in switch_pairs() {
            let chunk = [b"x".as_slice(), brace, enter, b"\x1b]133;A\x07"].concat();

            // `term_core` records the mark: it never left the main screen.
            let mut core = new_core();
            core.process_pty_data_fully(&chunk);
            assert_eq!(core.take_prompt_marks().len(), 1, "{chunk:?}");

            let extraction = extract_main_buffer(&chunk, false, DesignatorSlot::Ground);
            let mut scanner = AgentStatusFeedScanner::new();
            let items = scanner.feed(&chunk, &extraction.spans);
            assert!(
                matches!(items.as_slice(), [AgentStatusFeedItem::Osc133Mark(_)]),
                "{chunk:?}: the mark is live ({} items)",
                items.len()
            );
        }
    }
}

// ── AC-4: the carried state is O(1) and the pass stays linear ────────────

const BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

/// AC-4 (NFR3): the state a read leaves for the next one, for the shapes
/// that decide it: `ESC (` / `ESC )` at the end awaits a designator; a live
/// `ESC` at the end waits for a brace; an `ESC` taken as a designator is
/// neither.
#[test]
fn the_designator_slot_state_after_a_read() {
    use DesignatorSlot::{Awaiting, EscapeSeen, Ground};
    let cases: &[(DesignatorSlot, &[u8], DesignatorSlot)] = &[
        (Ground, b"ab", Ground),
        (Ground, b"a\x1b", EscapeSeen),
        (Ground, b"a\x1b\x1b", EscapeSeen),
        (Ground, b"a\x1b(", Awaiting),
        (Ground, b"a\x1b)", Awaiting),
        (Ground, b"a\x1b(B", Ground),
        (Ground, b"a\x1b(B\x1b", EscapeSeen),
        (Ground, b"a\x1b(B\x1b(", Awaiting),
        (Ground, b"a\x1b(\x1b", Ground),
        (Ground, b"a\x1b(\x1b(", Ground),
        (Ground, b"a\x1b(\x1b\x1b", EscapeSeen),
        (Ground, b"a\x1b(\x1b\x1b(", Awaiting),
        (Ground, b"a\x1b(\x1b(\x1b(", Awaiting),
        (Ground, b"a\x1b(\x1b(\x1b(\x1b", Ground),
        (Ground, b"a\x1b[?47h", Ground),
        (Ground, b"a\x1b[?47h\x1b(", Awaiting),
        (Ground, b"a\x1b[1m", Ground),
        (Awaiting, b"\x1b", Ground),
        (Awaiting, b"B", Ground),
        (Awaiting, b"B\x1b", EscapeSeen),
        (Awaiting, b"B\x1b(", Awaiting),
        (Awaiting, b"\x1b\x1b(", Awaiting),
        (Awaiting, b"\x1b(", Ground),
        (EscapeSeen, b"(", Awaiting),
        (EscapeSeen, b")", Awaiting),
        (EscapeSeen, b"(x", Ground),
        (EscapeSeen, b"(\x1b", Ground),
        (EscapeSeen, b")\x1b(", Ground),
        (EscapeSeen, b"[", Ground),
        (EscapeSeen, b"x", Ground),
        (EscapeSeen, b"\x1b", EscapeSeen),
        (EscapeSeen, b"\x1b(", Awaiting),
    ];
    for (start, chunk, expected) in cases {
        let got = extract_main_buffer(chunk, false, *start);
        assert_eq!(
            got.slot,
            *expected,
            "state {start:?} then {:?}",
            String::from_utf8_lossy(chunk)
        );
    }
}

/// AC-4 (NFR3): the state left after a stream does not depend on where the
/// reads end: whole, split once anywhere and one byte at a time all leave the
/// same slot state, switch sequences cut in two included.
#[test]
fn the_designator_slot_state_is_split_invariant() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for round in 0..3_000 {
        let stream = random_stream(&mut rng, FRAGMENTS, 14);
        let end_state = |pieces: &[&[u8]]| {
            let mut slot = DesignatorSlot::Ground;
            for piece in pieces {
                slot = extract_main_buffer(piece, false, slot).slot;
            }
            slot
        };
        let whole = end_state(&[&stream]);
        for at in 1..stream.len() {
            assert_eq!(
                end_state(&[&stream[..at], &stream[at..]]),
                whole,
                "round {round}: stream {:?} split at {at}",
                String::from_utf8_lossy(&stream)
            );
        }
        let singles: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(
            end_state(&singles),
            whole,
            "round {round}: stream {:?} byte by byte",
            String::from_utf8_lossy(&stream)
        );
    }
}

/// AC-4 (TM-2, NFR3, NFR5): adversarial `ESC (` chains interleaved with
/// switch sequences, in one chunk and fed one byte at a time, finish within
/// the budget, never panic and agree between the two ways of feeding.
#[test]
fn designator_chains_with_switches_stay_linear_in_extraction() {
    let mut rng = Rng(0x0dd_c0ff_ee15_600d);
    let units: [&[u8]; 8] = [
        b"\x1b(",
        b"\x1b(\x1b",
        b"\x1b)\x1b)",
        b"\x1b[?47l",
        b"\x1b[?1049h",
        b"\x1b[?1049l",
        b"\x1b[?1047h",
        b"\x1b(B",
    ];
    let mut stream = Vec::new();
    while stream.len() < 2 * 1024 * 1024 {
        stream.extend_from_slice(units[rng.below(units.len())]);
    }

    let start = std::time::Instant::now();
    let whole = extract_main_buffer(&stream, false, DesignatorSlot::Ground);
    assert!(
        start.elapsed() < BUDGET,
        "one chunk of {} bytes took {:?}",
        stream.len(),
        start.elapsed()
    );
    let total_removed: usize = stream.len() - whole.bytes.len();
    assert!(
        total_removed > 0,
        "some switches are removed from the chain stream"
    );

    let prefix = &stream[..300_000];
    let start = std::time::Instant::now();
    let mut slot = DesignatorSlot::Ground;
    for byte in prefix {
        slot = extract_main_buffer(std::slice::from_ref(byte), false, slot).slot;
    }
    assert!(
        start.elapsed() < BUDGET,
        "{} one-byte chunks took {:?}",
        prefix.len(),
        start.elapsed()
    );
    assert_eq!(
        slot,
        extract_main_buffer(prefix, false, DesignatorSlot::Ground).slot,
        "byte by byte and whole leave the same state"
    );
}

// ── AC-4: the reader under adversarial designator chains ─────────────────

/// Units of an adversarial stream. Each ends with the shadow parser on the
/// main screen whatever its neighbours are (every `h` form that is a switch
/// for the shadow parser is followed by an `l` in the same unit), so the
/// reader's ring can be judged against the term_core oracle alone. No unit is
/// a lone `ESC`: `ESC ESC` before a switch is a separate finding (FR1), not
/// part of this path.
const READER_UNITS: &[&[u8]] = &[
    b"\x1b(\x1b[?47l",
    b"\x1b[?47h\x1b[?47l",
    b"\x1b(B\x1b[?1049h\x1b[?1049l",
    b"\x1b(\x1b\x1b[?47h\x1b[?47l",
    b"\x1b)\x1b)\x1b[?1049h\x1b[?1049l",
    b"\x1b(\x1b(\x1b[?47l",
    b"\x1b)\x1b[?1047l",
    b"\x1b(",
    b"\x1b)\x1b)\x1b)",
    b"x",
];

/// `units` units chosen at random, grouped into reads of at most
/// `max_read` bytes that end at unit boundaries (a switch sequence is never
/// cut in two by a read), and the whole stream.
fn reader_stream(rng: &mut Rng, units: usize, max_read: usize) -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut reads: Vec<Vec<u8>> = vec![Vec::new()];
    let mut stream = Vec::new();
    for _ in 0..units {
        let unit = READER_UNITS[rng.below(READER_UNITS.len())];
        if reads.last().unwrap().len() + unit.len() > max_read {
            reads.push(Vec::new());
        }
        reads.last_mut().unwrap().extend_from_slice(unit);
        stream.extend_from_slice(unit);
    }
    (reads, stream)
}

/// AC-4 (TM-2, NFR3, NFR5): through the production reader, chains of
/// `ESC (` / `ESC )` interleaved with switch sequences finish within the
/// budget, never panic and leave a ring whose replay through `term_core`
/// equals the raw-stream reference — in a single read, in reads of several
/// kilobytes and with one unit per read (so `ESC (` ends reads over and over).
/// (The ring may drop an `ESC` that `term_core` superseded right before a
/// removed switch, so the ring is judged by its replay, not byte by byte.)
#[test]
fn adversarial_designator_chains_through_the_reader_finish_within_the_budget() {
    let mut rng = Rng(0x7f4a_7c15_9e37_79b9);

    // One read.
    let (reads, _stream) = reader_stream(&mut rng, 4_000, 60_000);
    assert_eq!(reads.len(), 1, "the stream fits one read");
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(reads.clone());
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_ring_replay_equals_reference(&ring, &reads, "one read");

    // Reads of several kilobytes.
    let (reads, _stream) = reader_stream(&mut rng, 30_000, 8_000);
    assert!(reads.len() > 10);
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(reads.clone());
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_ring_replay_equals_reference(&ring, &reads, "several reads");

    // One unit per read.
    let units = 20_000usize;
    let reads: Vec<Vec<u8>> = (0..units)
        .map(|_| READER_UNITS[rng.below(READER_UNITS.len())].to_vec())
        .collect();
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(reads.clone());
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_ring_replay_equals_reference(&ring, &reads, "one unit per read");
}

/// AC-4 (NFR5): through the production visibility restore, an adversarial
/// stream sends no empty `PtyOutput` chunk other than the EOF marker, and
/// the client the snapshot, the replacement and the later reads leave equals
/// the raw-stream reference.
#[test]
fn adversarial_designator_chains_send_no_empty_chunk_and_match_the_reference() {
    let mut rng = Rng(0x5851_f42d_4c95_7f2d);
    for round in 0..6 {
        let (reads, _stream) = reader_stream(&mut rng, 150, 120);
        assert!(reads.len() >= 3);
        for restore_read in [0, reads.len() / 2, reads.len() - 1] {
            let start = std::time::Instant::now();
            let run = run_visibility_restore_at(&reads, restore_read);
            assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
            let (eof, rest) = run.received.split_last().expect("deliveries");
            assert!(eof.data.is_empty(), "the last chunk is the EOF marker");
            for chunk in rest {
                assert!(
                    !chunk.data.is_empty(),
                    "round {round}: every chunk other than the EOF marker is non-empty"
                );
            }
            assert_client_equals_reference(
                &run.received,
                &reads,
                &format!("round {round}, restore at read {restore_read}"),
            );
        }
    }
}

// ── AC-6 (FR5, FR6): the decision record ─────────────────────────────────

/// The text of the section that starts at the line `heading` and runs to the
/// next heading of the same or a higher level.
fn section_of<'a>(doc: &'a str, heading: &str) -> &'a str {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let mut offset = 0usize;
    let mut start = None;
    let mut end = doc.len();
    for line in doc.lines() {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        let is_heading = hashes > 0 && line[hashes..].starts_with(' ');
        match start {
            None if line == heading => start = Some(offset),
            Some(_) if is_heading && hashes <= level => {
                end = offset;
                break;
            }
            _ => {}
        }
        offset += line.len() + 1;
    }
    let start = start.unwrap_or_else(|| panic!("DECISIONS.md has no heading {heading:?}"));
    &doc[start..end]
}

/// AC-6 (FR5, FR6, SPEC AC-6): the decision record's FR5 section states
/// whether the path reproduced, with the registry test path, names the
/// residual outside the fix boundary and its cause, and its
/// behavior-changing-tests subsection lists the existing tests whose
/// expectation this task changed. Every name the record cites resolves to a
/// test in the source.
#[test]
fn round4_fr5_the_decision_record_states_the_outcome_the_residual_and_the_changed_tests() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the decision record {} is unreadable: {e}", path.display()));

    let adjacent = section_of(&doc, "### FR5: switch sequence in the designator slot");
    assert!(
        !adjacent.contains("pending (task0005)"),
        "the FR5 section is filled in: {adjacent}"
    );
    let fixed = adjacent.contains("reproduced and fixed");
    let not_reproducing = adjacent.contains("not reproducing");
    assert!(
        fixed != not_reproducing,
        "the FR5 section states exactly one outcome: {adjacent}"
    );
    let registry_file = "src-tauri/src/mux/ipc/pty_spawn/tests/round4_designator_slot.rs";
    let registry_test = "mux::ipc::pty_spawn::tests::round4_designator_slot::\
                         round4_fr5_switch_in_the_designator_slot_matches_the_raw_stream_reference";
    assert!(adjacent.contains(registry_file), "{adjacent}");
    assert!(adjacent.contains(registry_test), "{adjacent}");
    // The cited path resolves: this module, and a test function of that name.
    let (module, function) = registry_test.rsplit_once("::").unwrap();
    assert!(module_path!().ends_with(module), "{}", module_path!());
    assert!(
        include_str!("round4_designator_slot.rs").contains(&format!("fn {function}()")),
        "{function} is defined in {registry_file}"
    );
    // The pre-change test failed (task0005 record) and the fix is in the
    // production path; a record that says otherwise contradicts it.
    assert!(
        fixed,
        "the pre-change test failed (task0005 record), so the outcome is `reproduced and fixed`"
    );
    // The residual outside the boundary: named, with its cause.
    let residual = adjacent
        .lines()
        .find(|line| line.starts_with("Residual:"))
        .unwrap_or_else(|| panic!("the FR5 section has a Residual line: {adjacent}"));
    for needle in ["shadow parser", "snapshot assembly", "D4"] {
        assert!(
            residual.contains(needle),
            "the residual names {needle:?}: {residual}"
        );
    }

    let behavior_changing = {
        let start = doc
            .find("## Behavior-changing tests")
            .expect("the Behavior-changing tests section");
        section_of(&doc[start..], "### FR5")
    };
    assert!(
        !behavior_changing.contains("pending (task0005)"),
        "the FR5 subsection is filled in: {behavior_changing}"
    );
    let changed = [
        "round4_48caec6f_awaiting_designator_at_a_cut_writes_the_consumed_esc",
        "round4_many_straddling_switches_after_a_waiting_designator_stay_within_the_budget",
    ];
    for name in changed {
        assert!(
            behavior_changing.contains(&format!("`{name}`")),
            "the FR5 subsection lists {name}: {behavior_changing}"
        );
        assert!(
            include_str!("round4_designator_cut.rs").contains(&format!("fn {name}()")),
            "the listed test exists under that name"
        );
    }
    assert!(
        behavior_changing
            .contains("src-tauri/src/mux/ipc/pty_spawn/tests/round4_designator_cut.rs"),
        "{behavior_changing}"
    );
}
