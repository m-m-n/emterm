//! Trailing-construct decider for a snapshot payload (mux-suppressed-output-
//! round2-fixes task0004, FR8).
//!
//! A snapshot payload is replayed by the client into a freshly reset parser.
//! When the payload ends in the middle of a construct (a cut UTF-8 character,
//! `ESC (` awaiting its designator byte, or a cut CSI), the client parser is
//! left holding that construct. [`trailing_construct`] reports which one, so
//! the suppressed-chunk replacement builder can avoid re-sending a tail the
//! client already holds.
//!
//! This is a leaf module: it depends on nothing in `crate::mux` and on no
//! `gui`-gated item.

/// The incomplete construct a snapshot payload leaves the client parser in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::mux) enum TrailingConstruct {
    /// The lead byte plus the continuation bytes seen so far.
    Utf8Partial(Vec<u8>),
    /// `ESC (` or `ESC )`, awaiting the designator byte.
    AwaitingDesignator(Vec<u8>),
    /// A CSI from its ESC to the payload end, with C0 bytes removed.
    IncompleteCsi(Vec<u8>),
}

impl TrailingConstruct {
    pub(in crate::mux) fn bytes(&self) -> &[u8] {
        match self {
            TrailingConstruct::Utf8Partial(b)
            | TrailingConstruct::AwaitingDesignator(b)
            | TrailingConstruct::IncompleteCsi(b) => b,
        }
    }

    pub(in crate::mux) fn into_bytes(self) -> Vec<u8> {
        match self {
            TrailingConstruct::Utf8Partial(b)
            | TrailingConstruct::AwaitingDesignator(b)
            | TrailingConstruct::IncompleteCsi(b) => b,
        }
    }
}

/// Whether `bytes` is exactly an awaiting-designator construct
/// (`ESC (` / `ESC )`).
pub(in crate::mux) fn is_awaiting_designator(bytes: &[u8]) -> bool {
    matches!(bytes, [0x1b, b'('] | [0x1b, b')'])
}

/// The longest construct the decider reports; a longer one gives none, so
/// the boundary record stays small and conflicting information falls back
/// to the pre-feature re-send.
const MAX_CONSTRUCT_LEN: usize = 256;

const ESC: u8 = 0x1b;

#[cfg(test)]
thread_local! {
    /// Test-only: called at the start of every [`trailing_construct`] on
    /// this thread, so a test can observe the caller's lock state at the
    /// moment the decider runs.
    pub(in crate::mux) static ENTRY_HOOK: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// The client parser's state machine, reduced to what decides the end
/// state. Transitions mirror `term_core`'s parser byte for byte
/// (IMPLEMENTATION.md D3); `term_core` itself is not a dependency of this
/// module.
#[derive(Clone, Copy)]
enum State {
    Ground,
    /// A UTF-8 character in progress: the lead byte's index, the bytes seen
    /// so far (lead included) and the bytes the character needs.
    Utf8 {
        start: usize,
        have: u8,
        need: u8,
    },
    Escape,
    /// After `ESC (` / `ESC )`: the next byte, whatever it is, is the
    /// designator.
    Designator(u8),
    CsiEntry,
    CsiParam,
    Osc,
    OscEscape,
    Apc,
    ApcEscape,
    Dcs,
    DcsEscape,
}

/// One byte in the ground state. `esc_start` receives the index of an ESC.
fn ground_step(byte: u8, index: usize, esc_start: &mut usize) -> State {
    match byte {
        ESC => {
            *esc_start = index;
            State::Escape
        }
        0xc0..=0xdf => State::Utf8 {
            start: index,
            have: 1,
            need: 2,
        },
        0xe0..=0xef => State::Utf8 {
            start: index,
            have: 1,
            need: 3,
        },
        0xf0..=0xf7 => State::Utf8 {
            start: index,
            have: 1,
            need: 4,
        },
        _ => State::Ground,
    }
}

/// One byte right after an ESC. `esc_start` is the index of the ESC that
/// opened the construct (updated by a further ESC); `csi_start` receives it
/// when a CSI opens.
fn escape_step(byte: u8, index: usize, esc_start: &mut usize, csi_start: &mut usize) -> State {
    match byte {
        b'[' => {
            *csi_start = *esc_start;
            State::CsiEntry
        }
        b']' => State::Osc,
        b'_' => State::Apc,
        b'P' => State::Dcs,
        b'(' | b')' => State::Designator(byte),
        ESC => {
            *esc_start = index;
            State::Escape
        }
        _ => State::Ground,
    }
}

/// The byte after an ESC that was seen inside a string: `\` ends the
/// string, any other byte ends it too and is processed as a fresh escape
/// whose ESC is the previous byte.
fn string_escape_step(
    byte: u8,
    index: usize,
    esc_start: &mut usize,
    csi_start: &mut usize,
) -> State {
    if byte == b'\\' {
        return State::Ground;
    }
    *esc_start = index.saturating_sub(1);
    escape_step(byte, index, esc_start, csi_start)
}

/// Decide the trailing construct of `payload`: the incomplete construct the
/// client parser is left in after replaying `payload` from a reset parser.
///
/// `payload` is the bytes of all segments in order; the client's parser
/// state carries across segment edges, so the segments are one byte string
/// here. One forward pass, no panicking arithmetic or indexing.
pub(in crate::mux) fn trailing_construct(payload: &[u8]) -> Option<TrailingConstruct> {
    #[cfg(test)]
    ENTRY_HOOK.with(|hook| {
        if let Some(f) = hook.borrow().as_ref() {
            f();
        }
    });
    let mut state = State::Ground;
    let mut esc_start = 0usize;
    let mut csi_start = 0usize;
    for (i, &b) in payload.iter().enumerate() {
        state = match state {
            State::Ground => ground_step(b, i, &mut esc_start),
            State::Utf8 { start, have, need } => {
                if (0x80..=0xbf).contains(&b) {
                    if have.saturating_add(1) >= need {
                        State::Ground
                    } else {
                        State::Utf8 {
                            start,
                            have: have + 1,
                            need,
                        }
                    }
                } else {
                    // A non-continuation byte ends the partial silently and
                    // is processed in the ground state.
                    ground_step(b, i, &mut esc_start)
                }
            }
            State::Escape => escape_step(b, i, &mut esc_start, &mut csi_start),
            State::Designator(_) => State::Ground,
            State::CsiEntry => match b {
                b'0'..=b'9' | b'?' | b'>' | b'<' | b'=' | b' ' | b';' | b':' => State::CsiParam,
                0x40..=0x7e => State::Ground,
                0x00..=0x1a | 0x1c..=0x1f => State::CsiEntry,
                ESC => {
                    esc_start = i;
                    State::Escape
                }
                _ => State::Ground,
            },
            State::CsiParam => match b {
                b'0'..=b'9' | b';' | b':' | 0x20..=0x2f => State::CsiParam,
                0x40..=0x7e => State::Ground,
                0x00..=0x1a | 0x1c..=0x1f => State::CsiParam,
                ESC => {
                    esc_start = i;
                    State::Escape
                }
                _ => State::Ground,
            },
            State::Osc => match b {
                0x07 => State::Ground,
                ESC => State::OscEscape,
                _ => State::Osc,
            },
            State::Apc => {
                if b == ESC {
                    State::ApcEscape
                } else {
                    State::Apc
                }
            }
            State::Dcs => {
                if b == ESC {
                    State::DcsEscape
                } else {
                    State::Dcs
                }
            }
            State::OscEscape | State::ApcEscape | State::DcsEscape => {
                string_escape_step(b, i, &mut esc_start, &mut csi_start)
            }
        };
    }

    match state {
        State::Utf8 { start, .. } => {
            let bytes = payload.get(start..)?;
            Some(TrailingConstruct::Utf8Partial(bytes.to_vec()))
        }
        State::Designator(designator) => {
            Some(TrailingConstruct::AwaitingDesignator(vec![ESC, designator]))
        }
        State::CsiEntry | State::CsiParam => incomplete_csi(payload, csi_start),
        _ => None,
    }
}

/// The incomplete CSI from `start` to the payload end with C0 bytes removed
/// (the removal the replacement builder applies to a CSI tail), or `None`
/// when it is longer than [`MAX_CONSTRUCT_LEN`].
fn incomplete_csi(payload: &[u8], start: usize) -> Option<TrailingConstruct> {
    let mut out = Vec::with_capacity(16);
    for &b in payload.get(start..)? {
        if matches!(b, 0x00..=0x1a | 0x1c..=0x1f) {
            continue;
        }
        if out.len() >= MAX_CONSTRUCT_LEN {
            return None;
        }
        out.push(b);
    }
    Some(TrailingConstruct::IncompleteCsi(out))
}

/// Convenience for the boundary record: the construct's bytes, or `None`.
pub(in crate::mux) fn trailing_construct_bytes(payload: &[u8]) -> Option<Vec<u8>> {
    trailing_construct(payload).map(TrailingConstruct::into_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_core::terminal_core::{ReplaySegment, TerminalCore};

    const ESC: u8 = 0x1b;

    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// Replay `segments` (each `(bytes, cols, rows)`) through term_core's
    /// `reset_and_replay_segments` as one payload, then feed `probe`, and
    /// return the text of row 0. This is the oracle: it observes the parse
    /// state term_core actually reaches at the end of the payload.
    fn replay_then_probe(segments: &[(&[u8], u16, u16)], probe: &[u8]) -> String {
        let mut payload = Vec::new();
        let mut replay = Vec::new();
        for (bytes, cols, rows) in segments {
            replay.push(ReplaySegment {
                offset: payload.len() as u32,
                cols: *cols,
                rows: *rows,
            });
            payload.extend_from_slice(bytes);
        }
        let mut core = TerminalCore::new(80, 24, 100);
        core.reset_and_replay_segments(&payload, &replay);
        core.process_pty_data_fully(probe);
        core.get_line_text(0).trim_end().to_string()
    }

    fn concat_segments(segments: &[(&[u8], u16, u16)]) -> Vec<u8> {
        segments
            .iter()
            .flat_map(|(b, _, _)| b.iter().copied())
            .collect()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        None,
        Utf8,
        Designator,
        Csi,
    }

    fn classify(payload: &[u8]) -> (Kind, Vec<u8>) {
        match trailing_construct(payload) {
            None => (Kind::None, Vec::new()),
            Some(TrailingConstruct::Utf8Partial(b)) => (Kind::Utf8, b),
            Some(TrailingConstruct::AwaitingDesignator(b)) => (Kind::Designator, b),
            Some(TrailingConstruct::IncompleteCsi(b)) => (Kind::Csi, b),
        }
    }

    struct Case {
        name: &'static str,
        /// One or more segments; a construct may straddle a segment edge.
        segments: Vec<Vec<u8>>,
        kind: Kind,
        bytes: Vec<u8>,
        /// Bytes fed to term_core after the replay, and the row-0 text that
        /// proves term_core is in the state the decider claims. `None`
        /// skips the oracle for a case where the decider deliberately
        /// reports less than the client holds (over-long construct).
        probe: Option<(&'static [u8], &'static str)>,
    }

    fn table() -> Vec<Case> {
        let mut long_csi = vec![ESC, b'['];
        long_csi.extend(std::iter::repeat_n(b'1', 300));
        let mut cases = vec![
            // ---- AC-1: UTF-8 partial (2-, 3-, 4-byte leads) ----
            Case {
                name: "2-byte lead, no continuation",
                segments: vec![cat(&[b"ab", &[0xc3]])],
                kind: Kind::Utf8,
                bytes: vec![0xc3],
                probe: Some((&[0xa9], "ab\u{e9}")),
            },
            Case {
                name: "3-byte lead, no continuation",
                segments: vec![cat(&[b"ab", &[0xe4]])],
                kind: Kind::Utf8,
                bytes: vec![0xe4],
                probe: Some((&[0xb8, 0xad], "ab\u{4e2d}")),
            },
            Case {
                name: "3-byte lead, one continuation",
                segments: vec![cat(&[b"ab", &[0xe4, 0xb8]])],
                kind: Kind::Utf8,
                bytes: vec![0xe4, 0xb8],
                probe: Some((&[0xad], "ab\u{4e2d}")),
            },
            Case {
                name: "4-byte lead, two continuations",
                segments: vec![cat(&[b"ab", &[0xf0, 0x90, 0x80]])],
                kind: Kind::Utf8,
                bytes: vec![0xf0, 0x90, 0x80],
                probe: Some((&[0x80], "ab\u{10000}")),
            },
            Case {
                name: "complete 3-byte character is not a construct",
                segments: vec![cat(&[b"ab", "\u{4e2d}".as_bytes()])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((&[0xad], "ab\u{4e2d}\u{fffd}")),
            },
            Case {
                name: "utf8 partial ended silently by a non-continuation byte",
                segments: vec![cat(&[b"ab", &[0xe4, 0xb8], b"c"])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((&[0xad], "abc\u{fffd}")),
            },
            Case {
                name: "utf8 partial then ESC restarts from the ESC",
                segments: vec![cat(&[b"ab", &[0xe4], &[ESC, b'[', b'1']])],
                kind: Kind::Csi,
                bytes: vec![ESC, b'[', b'1'],
                probe: Some((b";5HX", "ab  X")),
            },
            // ---- AC-1: awaiting designator ----
            Case {
                name: "ESC ( awaiting designator",
                segments: vec![cat(&[b"ab", &[ESC, b'(']])],
                kind: Kind::Designator,
                bytes: vec![ESC, b'('],
                probe: Some((b"0q", "ab\u{2500}")),
            },
            Case {
                name: "ESC ) awaiting designator",
                segments: vec![cat(&[b"ab", &[ESC, b')']])],
                kind: Kind::Designator,
                bytes: vec![ESC, b')'],
                probe: Some((&[b'0', 0x0e, b'q'], "ab\u{2500}")),
            },
            Case {
                name: "ESC ( with the designator present is complete",
                segments: vec![cat(&[b"ab", &[ESC, b'(', b'B']])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "abZ")),
            },
            Case {
                name: "ESC ( ESC: the designator slot consumes the ESC",
                segments: vec![cat(&[b"ab", &[ESC, b'(', ESC]])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "abZ")),
            },
            Case {
                name: "ESC ( ESC ESC: designator consumed, then a lone ESC",
                segments: vec![cat(&[b"ab", &[ESC, b'(', ESC, ESC]])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
            Case {
                name: "ESC ESC ( : the second ESC starts the construct",
                segments: vec![cat(&[b"ab", &[ESC, ESC, b'(']])],
                kind: Kind::Designator,
                bytes: vec![ESC, b'('],
                probe: Some((b"0q", "ab\u{2500}")),
            },
            // ---- AC-1: incomplete CSI ----
            Case {
                name: "CSI introducer only",
                segments: vec![cat(&[b"ab", &[ESC, b'[']])],
                kind: Kind::Csi,
                bytes: vec![ESC, b'['],
                probe: Some((b"1;5HX", "ab  X")),
            },
            Case {
                name: "CSI 1 ;",
                segments: vec![cat(&[b"ab", &[ESC, b'[', b'1', b';']])],
                kind: Kind::Csi,
                bytes: vec![ESC, b'[', b'1', b';'],
                probe: Some((b"5HX", "ab  X")),
            },
            Case {
                name: "CSI 1 CR ; keeps the construct with the CR removed",
                segments: vec![cat(&[b"ab", &[ESC, b'[', b'1', b'\r', b';']])],
                kind: Kind::Csi,
                bytes: vec![ESC, b'[', b'1', b';'],
                probe: Some((b"5HX", "ab  X")),
            },
            Case {
                name: "CSI private marker parameter",
                segments: vec![cat(&[b"ab", &[ESC, b'[', b'?', b'2', b'5']])],
                kind: Kind::Csi,
                bytes: vec![ESC, b'[', b'?', b'2', b'5'],
                probe: Some((b"lX", "abX")),
            },
            Case {
                name: "complete CSI is not a construct",
                segments: vec![cat(&[b"ab", &[ESC, b'[', b'1', b'm']])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "abZ")),
            },
            Case {
                name: "CSI cancelled by an invalid byte is not a construct",
                segments: vec![cat(&[b"ab", &[ESC, b'[', b'1', 0x80]])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "abZ")),
            },
            Case {
                name: "CSI aborted by ESC restarts from the new ESC",
                segments: vec![cat(&[b"ab", &[ESC, b'[', b'1', b';', ESC, b'[', b'2']])],
                kind: Kind::Csi,
                bytes: vec![ESC, b'[', b'2'],
                probe: Some((b";5HX", "ab")),
            },
            // ---- AC-1: none ----
            Case {
                name: "ground",
                segments: vec![cat(&[b"ab\r\n"])],
                kind: Kind::None,
                bytes: vec![],
                probe: None,
            },
            Case {
                name: "empty payload",
                segments: vec![vec![]],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "Z")),
            },
            Case {
                name: "inside an OSC",
                segments: vec![cat(&[b"ab", &[ESC, b']', b'0', b';', b't']])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
            Case {
                name: "inside an OSC after its introducing ESC (lone ESC in a string)",
                segments: vec![cat(&[b"ab", &[ESC, b']', b'0', b';', b't', ESC]])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"\\Z", "abZ")),
            },
            Case {
                name: "inside a DCS",
                segments: vec![cat(&[b"ab", &[ESC, b'P', b'q', b'x']])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
            Case {
                name: "inside an APC",
                segments: vec![cat(&[b"ab", &[ESC, b'_', b'G', b'x']])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
            Case {
                name: "a DCS body ignores BEL and parentheses",
                segments: vec![cat(&[b"ab", &[ESC, b'P', b'q', b'\x07', b'(']])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
            Case {
                name: "lone ESC",
                segments: vec![cat(&[b"ab", &[ESC]])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
            Case {
                name: "ESC ( inside an OSC body without an aborting ESC is payload",
                segments: vec![cat(&[b"ab", &[ESC, b']', b'0', b';', b'(', b'x']])],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
            Case {
                name: "OSC aborted by ESC then ( opens the designator slot (D3)",
                segments: vec![cat(&[b"ab", &[ESC, b']', b'0', b';', b't', ESC, b'(']])],
                kind: Kind::Designator,
                bytes: vec![ESC, b'('],
                probe: Some((b"0q", "ab\u{2500}")),
            },
            Case {
                name: "DCS aborted by ESC then [ opens a CSI (D3)",
                segments: vec![cat(&[b"ab", &[ESC, b'P', b'q', ESC, b'[', b'1']])],
                kind: Kind::Csi,
                bytes: vec![ESC, b'[', b'1'],
                probe: Some((b";5HX", "ab  X")),
            },
            Case {
                name: "construct above 256 bytes is reported as none",
                segments: vec![long_csi],
                kind: Kind::None,
                bytes: vec![],
                probe: None,
            },
            // ---- AC-1: constructs that straddle a segment edge ----
            Case {
                name: "straddle: UTF-8 partial cut between two segments",
                segments: vec![cat(&[b"ab", &[0xe4]]), vec![0xb8]],
                kind: Kind::Utf8,
                bytes: vec![0xe4, 0xb8],
                probe: Some((&[0xad], "ab\u{4e2d}")),
            },
            Case {
                name: "straddle: ESC ( cut between ESC and (",
                segments: vec![cat(&[b"ab", &[ESC]]), vec![b'(']],
                kind: Kind::Designator,
                bytes: vec![ESC, b'('],
                probe: Some((b"0q", "ab\u{2500}")),
            },
            Case {
                name: "straddle: ESC ( then designator in the next segment",
                segments: vec![cat(&[b"ab", &[ESC, b'(']]), vec![b'B']],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "abZ")),
            },
            Case {
                name: "straddle: CSI cut with a C0 byte at the edge",
                segments: vec![cat(&[b"ab", &[ESC, b'[', b'1']]), vec![b'\r', b';']],
                kind: Kind::Csi,
                bytes: vec![ESC, b'[', b'1', b';'],
                probe: Some((b"5HX", "ab  X")),
            },
            Case {
                name: "straddle: OSC body cut between two segments",
                segments: vec![cat(&[b"ab", &[ESC, b']', b'0']]), vec![b';', b't']],
                kind: Kind::None,
                bytes: vec![],
                probe: Some((b"Z", "ab")),
            },
        ];
        // Keep the table order stable and drop nothing.
        cases.shrink_to_fit();
        cases
    }

    /// AC-1: every table case is decided as declared, for the payload built
    /// from its segments, and (where a probe is declared) term_core, replaying
    /// the same segments through `reset_and_replay_segments` with a different
    /// grid size per segment, is observably in the state the decider reports.
    #[test]
    fn ac1_decider_agrees_with_the_table_and_with_term_core_segment_replay() {
        for case in table() {
            let payload: Vec<u8> = case.segments.concat();
            let (kind, bytes) = classify(&payload);
            assert_eq!(kind, case.kind, "case {:?}: construct kind", case.name);
            assert_eq!(bytes, case.bytes, "case {:?}: construct bytes", case.name);
            if let Some((probe, expected_row)) = case.probe {
                // Alternate the grid size (columns and rows) between segments
                // so a resize sits on every segment edge.
                let sized: Vec<(&[u8], u16, u16)> = case
                    .segments
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let (cols, rows) = if i % 2 == 0 {
                            (80u16, 24u16)
                        } else {
                            (100, 30)
                        };
                        (s.as_slice(), cols, rows)
                    })
                    .collect();
                assert_eq!(
                    replay_then_probe(&sized, probe),
                    expected_row,
                    "case {:?}: term_core is not in the state the decider reports",
                    case.name
                );
            }
        }
    }

    /// AC-1: a construct cut at EVERY byte position of its payload (with a
    /// resize on the segment edge) leaves term_core in the same state the
    /// decider reports for the whole payload.
    #[test]
    fn ac1_every_split_position_of_each_construct_agrees_with_term_core() {
        let payloads: Vec<(Vec<u8>, Kind, &[u8], &str)> = vec![
            (
                cat(&[b"ab", &[0xe4, 0xb8]]),
                Kind::Utf8,
                &[0xad],
                "ab\u{4e2d}",
            ),
            (
                cat(&[b"ab", &[ESC, b'(']]),
                Kind::Designator,
                b"0q",
                "ab\u{2500}",
            ),
            (
                cat(&[b"ab", &[ESC, b'[', b'1', b'\r', b';']]),
                Kind::Csi,
                b"5HX",
                "ab  X",
            ),
            (
                cat(&[b"ab", &[ESC, b']', b'0', b';', b't']]),
                Kind::None,
                b"Z",
                "ab",
            ),
        ];
        for (payload, kind, probe, expected_row) in payloads {
            assert_eq!(classify(&payload).0, kind, "decider on {payload:?}");
            for cut in 0..=payload.len() {
                let (a, b) = payload.split_at(cut);
                assert_eq!(
                    replay_then_probe(&[(a, 80, 24), (b, 100, 30)], probe),
                    expected_row,
                    "cut at {cut} of {payload:?}: term_core state must not depend on \
                     the segment edge"
                );
            }
        }
    }

    /// AC-1 (TM-2): 2 MiB of random and adversarial bytes complete in one
    /// pass without panicking, and anything reported obeys the shape rules.
    #[test]
    fn ac1_two_mib_random_and_adversarial_payloads_complete_without_panicking() {
        const LEN: usize = 2 * 1024 * 1024;
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let random: Vec<u8> = (0..LEN).map(|_| (next() >> 24) as u8).collect();
        // Bytes drawn from the alphabet that moves the state machine.
        let alphabet: &[u8] = &[
            ESC, b'[', b']', b'(', b')', b'P', b'_', b'\\', 0x07, b';', b'?', b'1', 0x0d, 0xe4,
            0xb8, 0xad, 0x80, b'a',
        ];
        let biased: Vec<u8> = (0..LEN)
            .map(|_| alphabet[(next() as usize) % alphabet.len()])
            .collect();
        let adversarial: Vec<Vec<u8>> = vec![
            [ESC, b'['].repeat(LEN / 2),
            [ESC, ESC].repeat(LEN / 2),
            [ESC, b'(', ESC, b'('].repeat(LEN / 4),
            [ESC, b']'].repeat(LEN / 2),
            {
                let mut v = vec![ESC, b'['];
                v.resize(LEN, b'9');
                v
            },
            {
                let mut v = vec![0xf0];
                v.resize(LEN, 0x80);
                v
            },
            {
                let mut v = vec![ESC, b'['];
                v.extend(std::iter::repeat_n(b'\r', LEN - 3));
                v.push(b';');
                v
            },
        ];
        let start = std::time::Instant::now();
        for payload in [random, biased].into_iter().chain(adversarial) {
            assert_eq!(payload.len(), LEN);
            match trailing_construct(&payload) {
                None => {}
                Some(TrailingConstruct::Utf8Partial(b)) => {
                    assert!(
                        (1..=3).contains(&b.len()),
                        "utf8 partial length {}",
                        b.len()
                    );
                }
                Some(TrailingConstruct::AwaitingDesignator(b)) => {
                    assert!(is_awaiting_designator(&b));
                }
                Some(TrailingConstruct::IncompleteCsi(b)) => {
                    assert!(b.len() <= 256, "csi construct length {}", b.len());
                    assert_eq!(&b[..2], &[ESC, b'[']);
                    assert!(
                        !b.iter().any(|c| matches!(c, 0x00..=0x1a | 0x1c..=0x1f)),
                        "C0 bytes must be removed"
                    );
                }
            }
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "nine 2 MiB payloads must complete in a single linear pass each"
        );
    }

    /// The reported bytes never depend on how the caller obtained the
    /// payload; a helper sanity check that keeps `concat_segments` used.
    #[test]
    fn concatenated_segments_are_the_payload() {
        let segs: Vec<(&[u8], u16, u16)> = vec![(b"ab", 80, 24), (b"\x1b(", 80, 24)];
        let payload = concat_segments(&segs);
        assert_eq!(payload, b"ab\x1b(");
        assert!(is_awaiting_designator(
            trailing_construct(&payload).unwrap().bytes()
        ));
    }
}
