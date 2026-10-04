//! Scrollback rich-content filtering shared by the mux IPC reattach path and
//! the session pane resume path.
//!
//! This module is the single home for [`strip_replayable_rich_content`] so the
//! session layer (`mux::session::pane`) does not have to reach into the IPC
//! layer (`mux::ipc::reattach`) for it; both depend on this shared module
//! instead.

use crate::mux::osc_identify::{OscIdentity, osc_body_identity};

/// The CSI sub-states of `term_core`'s parser (`csi_entry` / `csi_param` in
/// `crates/term_core/src/parser/csi.rs`) the mux has to tell apart: they differ
/// in which bytes they accept and which they cancel on.
///
/// Defined here, in the shared strip module, so that this module never depends
/// on the IPC layer; the write filter (`mux::ipc::pty_spawn`) names it too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::mux) enum CsiPhase {
    /// Right after `ESC [`: a private marker (`< = > ?`) and a space are
    /// accepted, and the intermediates `0x21..=0x2F` cancel the CSI.
    Entry,
    /// After a parameter byte, separator, marker or space: the intermediates
    /// `0x20..=0x2F` are accepted, and a private marker cancels the CSI.
    Param,
}

/// What one byte does to a CSI the parser is inside ([`csi_step`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::mux) enum CsiStep {
    /// The byte is consumed and the CSI stays open in this sub-state.
    Open(CsiPhase),
    /// The byte is consumed and ends the CSI: it completes it (a final byte,
    /// `0x40..=0x7E`) or cancels it (a byte `term_core` treats as invalid).
    End,
    /// The byte is an `ESC`: it aborts the CSI and starts a new escape. It is
    /// not consumed by the CSI.
    Abort,
}

/// The single closing byte (DEL), defined once here: the strip writes it at a
/// removed construct when the written stream is inside a CSI or right after a
/// written lone `ESC` (mux-strip-concat-query-closure D1) and in place of the
/// final byte of a device query completed in a later call (D2); the write
/// filter writes it at a cut when the emitted stream ends inside a CSI
/// (mux-suppressed-output-round4-fixes FR4, D3).
///
/// In both CSI sub-states `term_core` treats DEL as invalid, cancels the CSI
/// without dispatching it and returns to ground, and right after a lone `ESC`
/// it ends the escape - no character, cursor move or response; in ground it is
/// ignored. It is not an `ESC`, so neither strip reads it as the start of a
/// strip target, and the CSI scan of the strip treats it as outside the CSI
/// grammar.
pub(in crate::mux) const CSI_CLOSING_BYTE: u8 = 0x7f;

/// [`CSI_CLOSING_BYTE`] as a slice, for writing it into a byte run.
pub(in crate::mux) const CSI_CLOSING: &[u8] = &[CSI_CLOSING_BYTE];

/// The write that closes an open OSC body or an open DCS / APC body, defined
/// once here (mux-write-filter-overflow-open-string-cut FR2, FR5, D2;
/// mux-strip-open-string-body-closure D1): `ESC` (0x1B) followed by CAN (0x18).
/// The write filter writes it at a cut when the written stream ends inside such a
/// body; the strip writes it at a removed construct while the written stream is
/// inside such a body ([`Written::close_before_removal`]).
///
/// `term_core` takes the `ESC` into the string's escape state, and the CAN as
/// the byte that aborts the string (the string is dispatched, an OSC as
/// `Unterminated`) and is processed as from the Escape state, where it is an
/// unknown escape final: the escape completes and the parser returns to ground,
/// without a character, a cursor move or a response of its own. That is what the
/// client's parser did at the removed switch's `ESC`, so a replay stands in
/// ground and the bytes written after the closure are never absorbed into the
/// string (a later BEL cannot complete it with them). The `ESC` is followed by
/// CAN, which opens no strip target, and `ESC` + CAN is not ST (`ESC \`), so
/// neither strip reads the closure as the start of a strip target or ends an
/// APC / DCS body at it. [`WrittenState`] takes both bytes from either body to
/// ground.
pub(in crate::mux) const STRING_BODY_CLOSING: &[u8] = &[0x1b, 0x18];

/// The per-byte CSI transition of `term_core` (`csi_entry` / `csi_param`),
/// defined once: the state-reporting strip
/// ([`strip_pty_output_for_scrollback_write_with_written_state`]) follows it, and
/// no second copy exists (the write filter's boundary scan does not walk CSI
/// bytes any more).
///
/// C0 controls other than `ESC` execute inside the CSI and leave its sub-state
/// alone.
pub(in crate::mux) fn csi_step(phase: CsiPhase, byte: u8) -> CsiStep {
    match (phase, byte) {
        (_, 0x1b) => CsiStep::Abort,
        (_, 0x00..=0x1a | 0x1c..=0x1f) => CsiStep::Open(phase),
        (_, 0x40..=0x7e) => CsiStep::End,
        (_, b'0'..=b'9' | b';' | b':') => CsiStep::Open(CsiPhase::Param),
        (CsiPhase::Entry, b'?' | b'>' | b'<' | b'=' | b' ') => CsiStep::Open(CsiPhase::Param),
        (CsiPhase::Param, 0x20..=0x2f) => CsiStep::Open(CsiPhase::Param),
        // Invalid in this state (DEL, a byte above 0x7F, an intermediate in
        // the entry state, a private marker after a parameter): cancelled
        // without dispatch.
        _ => CsiStep::End,
    }
}

/// The accumulation of a CSI's first parameter, defined once: the scan of a
/// complete candidate ([`scan_csi_device_query`]) and the classification of an
/// open CSI ([`OpenCsi`]) both feed their parameter bytes through it, so they
/// cannot drift apart.
///
/// The leading decimal run is accumulated with saturation, mirroring
/// `term_core`'s `ParamParser::add_digit` (`saturating_mul` / `saturating_add`,
/// `crates/term_core/src/parser_params.rs`): an arbitrarily long digit run can
/// never overflow or panic. A saturated value never equals a small target
/// constant (5/6/14/16/18), so an oversized parameter is simply preserved, never
/// stripped (the `n` arm of [`csi_is_device_query`] additionally clamps to
/// `term_core`'s `MAX_PARAM_VALUE` before truncating to `u8`). An empty run is
/// 0, as `ParamParser::get_first_or_zero` has it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FirstParam {
    value: u32,
    /// The leading digit run is still being collected: no `;` / `:` has been
    /// seen yet.
    collecting: bool,
}

impl FirstParam {
    const START: Self = Self {
        value: 0,
        collecting: true,
    };

    /// Take one parameter-region byte (`0x30..=0x3b`: a digit, `;` or `:`).
    /// Digits feed the first parameter while the leading run lasts; `;` / `:`,
    /// and a digit past the first run, end it. Intermediate bytes seen before
    /// do not matter (`term_core`'s digit arm has no such guard).
    #[inline]
    fn feed(&mut self, byte: u8) {
        if self.collecting && byte.is_ascii_digit() {
            self.value = self
                .value
                .saturating_mul(10)
                .saturating_add(u32::from(byte - b'0'));
        } else {
            self.collecting = false;
        }
    }
}

/// An O(1) description of the CSI the written stream is inside (mux-strip-
/// concat-query-closure): the sub-state, the private marker, the first
/// parameter and the first intermediate. Only written bytes advance it, through
/// the transitions of [`csi_step`] and the accumulation the scan uses
/// ([`FirstParam`]); the decision "this final byte completes an answered device
/// query" is [`csi_is_device_query`], unchanged.
///
/// It is what the write filter carries across calls instead of holding the CSI
/// bytes: a final byte that completes an answered query in a later call is
/// written as CSI_CLOSING in place of itself (D2), so the ring never holds an
/// executable query.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::mux) struct OpenCsi {
    phase: CsiPhase,
    /// The private marker (`< = > ?`), accepted only right after `ESC [`.
    marker: Option<u8>,
    first: FirstParam,
    /// The first intermediate byte (`0x20..=0x2F`); `csi_is_device_query` looks
    /// at whether there is one and, for DECRPM, at whether it is `$`.
    first_intermediate: Option<u8>,
}

impl OpenCsi {
    /// The CSI right after `ESC [`.
    fn entry() -> Self {
        Self {
            phase: CsiPhase::Entry,
            marker: None,
            first: FirstParam::START,
            first_intermediate: None,
        }
    }

    /// The CSI sub-state.
    pub(in crate::mux) fn phase(self) -> CsiPhase {
        self.phase
    }

    /// One written byte, by [`csi_step`]. The classification changes only when
    /// the CSI stays open; C0 controls other than `ESC` execute inside the CSI
    /// and leave it alone.
    #[inline]
    fn advance(&mut self, byte: u8) -> CsiStep {
        let step = csi_step(self.phase, byte);
        if let CsiStep::Open(next) = step {
            self.phase = next;
            match byte {
                0x20..=0x2f => {
                    self.first_intermediate.get_or_insert(byte);
                }
                0x30..=0x3b => self.first.feed(byte),
                // Only the entry state accepts a private marker.
                0x3c..=0x3f => self.marker = Some(byte),
                _ => {}
            }
        }
        step
    }

    /// Whether `byte`, written next, completes this CSI as a device query that
    /// `term_core` answers.
    #[inline]
    fn completes_an_answered_query(&self, byte: u8) -> bool {
        matches!(byte, 0x40..=0x7e)
            && csi_is_device_query(
                self.marker,
                self.first_intermediate.as_slice(),
                self.first.value,
                byte,
            )
    }
}

/// Where a parser replaying the bytes written so far stands, as far as the
/// closing at a cut needs it (mux-cut-csi-post-strip-closure D1,
/// mux-strip-escape-state-carry D1, mux-write-filter-overflow-open-string-cut
/// D1). In ground only `ESC` matters. An OSC body and a DCS / APC body that the
/// written bytes leave open are states of their own: the client's parser closed
/// such a string at a removed switch's `ESC`, so a replay of the ring must be
/// closed too, and the closure of a body differs from that of ground. The two
/// bodies end differently (BEL ends an OSC body and is data in a DCS / APC
/// body), so they are two states; in both, an `ESC` leaves the body for
/// [`WrittenState::Escape`], from which `\` completes ST and any other byte
/// aborts the string and is read as from that state.
///
/// The strip is in the same position as a cut: where it removes a construct
/// together with its opening `ESC` while the written stream is inside an OSC body
/// or a DCS / APC body, it writes [`STRING_BODY_CLOSING`] first
/// (mux-strip-open-string-body-closure D1), so the written stream leaves the
/// removal in ground.
///
/// Defined here, in the shared strip module, so that this module never depends
/// on the IPC layer; the write filter (`mux::ipc::pty_spawn`) carries it from one
/// call to the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::mux) enum WrittenState {
    Ground,
    /// Right after an `ESC` that was written.
    Escape,
    /// Right after `ESC (` / `ESC )`: the next byte is the charset designator,
    /// consumed whatever it is.
    Designator,
    /// Inside a CSI.
    Csi(OpenCsi),
    /// Inside an OSC body: after a written `ESC ]` and its body bytes. BEL
    /// returns to ground, `ESC` moves to [`WrittenState::Escape`], every other
    /// byte keeps the body.
    OscBody,
    /// Inside a DCS / APC body (the ST-terminated strings): after a written
    /// `ESC P` / `ESC _` and its body bytes. Only `ESC` leaves it, for
    /// [`WrittenState::Escape`]; BEL and every other byte are body data.
    StBody,
}

impl WrittenState {
    /// The transition of `term_core` for one written byte.
    #[inline]
    fn advance(&mut self, byte: u8) {
        *self = match *self {
            Self::Ground => {
                if byte == 0x1b {
                    Self::Escape
                } else {
                    return;
                }
            }
            Self::Escape => match byte {
                b'[' => Self::Csi(OpenCsi::entry()),
                b'(' | b')' => Self::Designator,
                // A string introducer enters the body of that string: `]` the
                // OSC body, `P` / `_` the ST-terminated (DCS / APC) body.
                b']' => Self::OscBody,
                b'P' | b'_' => Self::StBody,
                // Another ESC restarts the escape.
                0x1b => return,
                // Any other byte completes the escape. `\` is the ST that
                // completes a string whose `ESC` led here; the others are
                // escapes of one byte (`ESC X` / `ESC ^` among them: term_core
                // has no SOS / PM string). A byte that aborts a string is thus
                // read as from this state, so aborting adds no state.
                _ => Self::Ground,
            },
            Self::Designator => Self::Ground,
            // Only BEL and `ESC` leave an OSC body; every other byte is body
            // data.
            Self::OscBody => match byte {
                0x07 => Self::Ground,
                0x1b => Self::Escape,
                _ => return,
            },
            // Only `ESC` leaves a DCS / APC body; BEL is body data there.
            Self::StBody => match byte {
                0x1b => Self::Escape,
                _ => return,
            },
            Self::Csi(mut open) => match open.advance(byte) {
                CsiStep::Open(_) => Self::Csi(open),
                CsiStep::End => Self::Ground,
                CsiStep::Abort => Self::Escape,
            },
        };
    }

    /// The CSI sub-state: the phase inside a CSI, `None` in every other state.
    pub(in crate::mux) fn csi(self) -> Option<CsiPhase> {
        self.open_csi().map(OpenCsi::phase)
    }

    /// The open CSI with its classification, `None` outside a CSI.
    fn open_csi(self) -> Option<OpenCsi> {
        match self {
            Self::Csi(open) => Some(open),
            _ => None,
        }
    }
}

/// The output of a strip pass together with the [`WrittenState`] of the
/// stream it forms. Every byte the pass writes goes through here, so the state
/// is advanced inside the pass itself, once per written byte: bytes the pass
/// removes never advance it.
struct Written {
    bytes: Vec<u8>,
    state: WrittenState,
}

impl Written {
    /// Write one byte and advance the state by it.
    ///
    /// D2 (mux-strip-concat-query-closure FR3): a byte that completes the open
    /// CSI the written stream is inside as a device query `term_core` answers is
    /// written as [`CSI_CLOSING_BYTE`] in place of itself, one for one. Inside
    /// one pass this fires only for a CSI carried in from an earlier pass: a
    /// complete query whose bytes are all in the pass is already removed by
    /// [`scan_csi_device_query`]. A CSI that completes as a non-query is written
    /// unchanged.
    #[inline]
    fn push(&mut self, byte: u8) {
        if let WrittenState::Csi(open) = &self.state {
            if open.completes_an_answered_query(byte) {
                self.state = WrittenState::Ground;
                self.bytes.push(CSI_CLOSING_BYTE);
                return;
            }
        }
        self.state.advance(byte);
        self.bytes.push(byte);
    }

    fn extend(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.push(byte);
        }
    }

    /// D1 (mux-strip-concat-query-closure FR1, FR4; widened to the string
    /// bodies by mux-strip-open-string-body-closure FR1): called before the pass
    /// removes a construct together with its opening `ESC`. When the written
    /// stream is inside a CSI (entry or parameter state) or right after a
    /// written lone `ESC`, the bytes before the construct and the bytes after
    /// it would otherwise be joined: one [`CSI_CLOSING_BYTE`] is written first.
    /// It cancels the CSI, or ends the pending escape, in `term_core` without a
    /// display, cursor or response effect, and leaves the written stream in
    /// ground, so further constructs removed in the same run write nothing.
    ///
    /// When the written stream is inside an OSC body or a DCS / APC body, the
    /// client's parser ended that string at the removed construct's `ESC`, so the
    /// bytes after the construct would otherwise be absorbed into the string: it
    /// is closed first with [`STRING_BODY_CLOSING`] (`ESC` + CAN), written through
    /// [`Written::extend`], which leaves the written stream in ground. In ground,
    /// after a complete string and in a pending charset designator, nothing is
    /// written.
    ///
    /// It is written BEFORE any C0 byte re-emitted from a removed query: a C0
    /// byte right after a lone `ESC` would be consumed as that escape's final
    /// byte instead of executing, and a re-emitted BEL would end an open OSC
    /// body, completing it with the bytes the body already holds.
    #[inline]
    fn close_before_removal(&mut self) {
        match self.state {
            WrittenState::Csi(_) | WrittenState::Escape => self.push(CSI_CLOSING_BYTE),
            WrittenState::OscBody | WrittenState::StBody => self.extend(STRING_BODY_CLOSING),
            WrittenState::Ground | WrittenState::Designator => {}
        }
    }
}

/// CAN: the byte a vt100 parser cancels a CSI or an escape on.
const CAN_BYTE: u8 = 0x18;

/// The copy of a byte run that is handed to a vt100 parser (mux-vt100-del-closing
/// FR3, FR4).
///
/// `term_core` cancels a CSI on DEL ([`CSI_CLOSING_BYTE`]) and ends a lone `ESC`
/// on it, but a vt100 parser ignores DEL and keeps the sequence open, so the
/// next byte becomes its final byte. The ring (and everything the client's
/// `term_core` receives) keeps the single DEL; this copy is the one place where
/// it becomes CAN, which vt100 cancels on:
///
/// - An input DEL becomes CAN when the scan state just before it is inside a CSI
///   (entry or parameter sub-state) or right after an `ESC` ([`WrittenState::Csi`]
///   / [`WrittenState::Escape`]).
/// - A DEL in ground, in an OSC body, in a DCS / APC body ([`WrittenState::OscBody`]
///   / [`WrittenState::StBody`]) and while a charset designator is pending is
///   kept.
/// - Every other byte is copied unchanged, including a CAN already in the input.
///
/// The scan starts in ground and advances by each ORIGINAL input byte through
/// [`WrittenState`]'s transition, never by the replacing CAN: after a replaced
/// DEL the scan is in ground, so a second DEL right after it is kept. It does
/// not go through the strip's [`Written`] writer, so the D2 rewrite of an
/// answered query's final byte never applies: the copy differs from the input
/// only by DEL becoming CAN.
///
/// Returns a new vector of the input's length, in one pass with O(1) state
/// besides the output; it has no error case and does not panic. The ring and the
/// bytes sent to the client are not affected: call it only at the vt100
/// hand-off, right before a vt100 parser processes the bytes.
pub(in crate::mux) fn vt100_replay_copy(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut state = WrittenState::Ground;
    for &byte in bytes {
        let replace = byte == CSI_CLOSING_BYTE
            && matches!(state, WrittenState::Escape | WrittenState::Csi(_));
        out.push(if replace { CAN_BYTE } else { byte });
        state.advance(byte);
    }
    out
}

/// Remove rich-content viewer launch sequences from a completed byte run so a
/// reattach / window-switch snapshot replays plain-text history WITHOUT
/// re-spawning child WebView viewers or re-rendering inline images.
///
/// A pane's scrollback ring holds the raw PTY bytes a shell emitted, including
/// the sequences that originally triggered a viewer / inline image. Replaying
/// those verbatim on every reattach re-runs the side effect (e.g. `emterm
/// markdown` re-opens a Markdown WebView window). The fix is to strip those
/// launch sequences from the snapshot here; everything else (plain text, SGR,
/// cursor motion, `ESC[?1049h/l`, fold marks, status-bar OSC, titles, …) is
/// preserved byte-for-byte.
///
/// Removed:
/// - OSC 777 viewer launch: `ESC ] 777 ; emterm ; <kind> ; …` (BEL or ST
///   terminated) where `<kind>` is one of
///   [`crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS`]
///   (`markdown` / `image` / `json` / `yaml` / `html`). The OSC number and
///   data are recovered the way the client's parser reconstructs them
///   ([`crate::mux::osc_identify`]), so a leading-zero number (`0777;…`) or
///   non-digit bytes before the first `;` (`777emterm;;markdown;…`) are
///   stripped like the canonical spelling. `<kind> == fold` (fold marks)
///   and any other `<kind>` (status-bar, …) are KEPT. There is no `resize`
///   kind any more (task0004 round-4 rework D1'): dimensions travel
///   structurally alongside the payload
///   (`mux::scrollback_buffer::ScrollbackRingBuffer::read_segments` /
///   `mux_ipc::protocol::DimSegment`), never as an OSC 777 body in the byte
///   stream — see [`strip_pty_output_for_scrollback_write`]'s doc comment.
/// - Kitty graphics APC: `ESC _ G … ESC \`
/// - SIXEL DCS: `ESC P <params> q …  ESC \` (only DCS whose final byte is
///   `q`; a DCS whose *data* merely contains `q`, e.g. DECRQSS, is KEPT).
/// - emterm Markdown OSC 9999: `ESC ] 9999 ; emterm-md ; …` (BEL or ST
///   terminated; recovered the same way). `ESC ] 9999 ; emterm-mux ; …`
///   (mux control) is KEPT.
/// - OSC 777 agent-status reports (`ESC ] 777 ; emterm ; agent-status ; …`).
/// - CSI device queries that `crates/term_core/src/csi_dispatch.rs` answers
///   with a response, so a snapshot replay never makes the GUI synthesize a
///   stale reply: DSR / CPR (`ESC[5n`, `ESC[6n`), DA1 / DA2 (`ESC[c`,
///   `ESC[0c`, `ESC[?…c`, `ESC[>c`, `ESC[>0c`), XTWINOPS size reports
///   (`ESC[14t`, `ESC[16t`, `ESC[18t`), and DECRPM (`ESC[?Ps$p`). Any other
///   CSI — SGR, cursor motion, `ESC[?1049h/l`, DECSTBM, DA3 (`ESC[=c`,
///   unanswered), `ESC[0n` (unanswered `Ps`), non-size XTWINOPS, … — is
///   KEPT. See [`scan_csi_device_query`] for the exact predicate.
///
/// `bytes` is assumed to be a completed byte run (the scrollback ring stores
/// whole sequences). A sequence whose terminator never arrives is treated as
/// non-matching and left intact, so plain text is never accidentally dropped.
///
/// Where a construct is removed together with its opening `ESC` while the
/// written stream is inside a CSI, or right after a written lone `ESC`, one
/// CSI_CLOSING (DEL) is written at its position first, so the bytes before and
/// after it are never joined into an escape or a device query the raw stream
/// never made (mux-strip-concat-query-closure D1). Where it is removed while the
/// written stream is inside an OSC body or a DCS / APC body, STRING_BODY_CLOSING
/// (`ESC` + CAN) is written at its position first, so the bytes after it are
/// never absorbed into the string and completed by a later BEL into an OSC colour
/// query the raw stream never made (mux-strip-open-string-body-closure D1). In
/// ground, after a complete string and while a charset designator is pending,
/// nothing is added.
///
/// Runs in a single O(n) pass: once an `ESC \` (ST) terminator search runs off
/// the end of the buffer, that "no more ST terminators" fact is cached in
/// `st_search_from` so later APC / DCS introducers do not re-scan the tail
/// (which would make a buffer full of unterminated introducers quadratic). The
/// OSC terminator search is likewise bounded — it stops at the first bare ESC,
/// so it never scans past the introducer's own (short) run.
pub(in crate::mux) fn strip_replayable_rich_content(bytes: &[u8]) -> Vec<u8> {
    strip_rich_content(bytes)
}

/// Write-path alias for [`strip_replayable_rich_content`]
/// (`crate::mux::ipc::pty_spawn::ScrollbackWriteFilter` calls this name).
///
/// task0004 round-4 rework (D1'): rounds 1-3 had a separate write-path
/// variant here that ALSO stripped a `resize`-kind OSC 777 body (plus a
/// second, ANSI-context-free literal-byte-pattern pass closing forgeries
/// nested inside sequences the structural pass didn't fully consume) —
/// because a child process could emit the exact resize-marker byte
/// sequence, and dimensions were carried IN the byte stream. Every one of
/// those forgery findings across rounds 1-3 (`0c18ff55032328ab`,
/// `15c54fb74bb91ec7`, `95fb7c115b0b64da`, `4a22bd439fcdaf56`,
/// `d4a83d5403bf1d7c`) existed only because there was marker-shaped content
/// for PTY output to collide with. D1' moves dimensions OUT of the byte
/// stream entirely (see
/// `mux::scrollback_buffer::ScrollbackRingBuffer::read_segments` /
/// `mux_ipc::protocol::DimSegment`) — there is no more `resize` OSC kind,
/// no marker-shaped byte pattern, and nothing left to strip beyond what
/// [`strip_replayable_rich_content`] already strips. The write path and the
/// snapshot path are now IDENTICAL, so this is a plain alias rather than a
/// second implementation that could drift from it.
pub(in crate::mux) fn strip_pty_output_for_scrollback_write(bytes: &[u8]) -> Vec<u8> {
    strip_rich_content(bytes)
}

/// State-taking form of [`strip_pty_output_for_scrollback_write`]:
/// `pending_designator` is true when `bytes[0]` is the pending charset
/// designator of an `ESC (` / `ESC )` that ended the previous feed. See
/// [`strip_rich_content_and_remap_with_designator`]. The write filter uses
/// [`strip_pty_output_for_scrollback_write_with_written_state`], which returns
/// the same bytes.
#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::mux) fn strip_pty_output_for_scrollback_write_with_designator(
    bytes: &[u8],
    pending_designator: bool,
) -> Vec<u8> {
    strip_rich_content_and_remap_with_designator(bytes, &[], pending_designator).0
}

/// State-reporting form of [`strip_pty_output_for_scrollback_write_with_designator`]
/// for the write filter (mux-cut-csi-post-strip-closure D1, widened to the full
/// written state by mux-strip-escape-state-carry D1): the same single pass,
/// which also reports the end state of the stream formed by the state before
/// these bytes followed by the stripped bytes.
///
/// - `pending_designator`: `bytes[0]` is a pending charset designator (as in
///   the designator form). It alone decides the verbatim copy of the first byte;
///   the carried state never does.
/// - `state_in`: the end state of the written stream before `bytes`; inside a
///   CSI it carries the classification [`OpenCsi`] (sub-state, private marker,
///   first parameter and first intermediate).
///   Precondition: when `pending_designator` is set, `state_in` is
///   [`WrittenState::Designator`], or [`WrittenState::Ground`] when removals
///   spliced the stream so that the boundary scan awaits a designator the
///   written stream has already consumed (mux-strip-escape-state-carry EC-5).
///   Checked by a debug assertion.
///
/// Returns the stripped bytes and the state the written bytes leave the stream
/// in. The carried state influences no removal decision: the bytes differ from
/// what the designator form returns for the same bytes and flag only by what D1
/// and D2 write.
///
/// The state is advanced by every byte the pass writes (copied bytes,
/// designator bytes, the closing bytes and the C0 bytes re-emitted from a
/// removed CSI query, which execute without ending an open CSI; a re-emitted BEL
/// does end an open OSC body, as it does on a replay of the ring), and never by a
/// byte the pass removes. A removed construct's opening `ESC` therefore neither
/// aborts a CSI the written stream is inside nor completes an escape the written
/// stream is in, so the pass closes either first (D1,
/// [`Written::close_before_removal`]) and the stream leaves the removal in
/// ground; a byte that completes the carried CSI as an answered device query is
/// written as CSI_CLOSING in place of itself (D2, [`Written::push`]). The
/// opening `ESC` does not end or abort a string body the written stream is inside
/// either, so D1 closes that body too: it writes [`STRING_BODY_CLOSING`] first
/// (mux-strip-open-string-body-closure D1) and the stream leaves the removal in
/// ground. The transitions are `term_core`'s ([`csi_step`] and the escape and
/// string-body states of [`WrittenState`]); the body states are reported
/// whatever path wrote the bytes, not only an overflow flush. Extra state is
/// O(1); there is no second pass.
///
/// Started from an open CSI, the output may differ from the ground-started
/// strip of the carried bytes followed by `bytes` only by what D1 and D2 write:
/// the ground-started strip removes a query the carried CSI completes together
/// with the carried bytes, the state-reporting form (which does not see the
/// carried bytes) closes the CSI in place of the final byte.
pub(in crate::mux) fn strip_pty_output_for_scrollback_write_with_written_state(
    bytes: &[u8],
    pending_designator: bool,
    state_in: WrittenState,
) -> (Vec<u8>, WrittenState) {
    debug_assert!(
        !pending_designator || matches!(state_in, WrittenState::Designator | WrittenState::Ground),
        "a pending designator is only given with a Designator or Ground state, got {state_in:?}"
    );
    let (out, _, state) = strip_pass(bytes, &[], pending_designator, state_in);
    (out, state)
}

/// Shared implementation for [`strip_replayable_rich_content`] /
/// [`strip_pty_output_for_scrollback_write`] (the write path and the
/// snapshot path are identical since task0004 round-4 rework D1' — see
/// [`strip_pty_output_for_scrollback_write`]'s doc comment for why).
fn strip_rich_content(bytes: &[u8]) -> Vec<u8> {
    strip_rich_content_and_remap(bytes, &[]).0
}

/// [`strip_rich_content`], plus remapping of `watch_offsets` (byte positions
/// into the ORIGINAL `bytes`, in ascending order) to their corresponding
/// position in the returned, stripped output — a single O(n + m) pass
/// (`m = watch_offsets.len()`), not a second scan per offset.
///
/// Used by `mux::snapshot_bytes` (task0004 round-4 rework, D1') to keep
/// structural dimension segments (offsets recorded against a pane's RAW
/// scrollback bytes) valid after this strip removes bytes ahead of them —
/// without it, a segment's `offset` would point past whatever content the
/// strip removed before it, misaligning every later segment.
///
/// An offset at the first byte of a sequence this pass REMOVES maps to the
/// output position before whatever the removal writes (the closure D1 inserts:
/// a closing byte or [`STRING_BODY_CLOSING`], see
/// [`Written::close_before_removal`]). An offset strictly inside the
/// removed span, or right after it, maps to the output position after the
/// closure and the C0 bytes re-emitted from a removed query (mux-strip-
/// concat-query-closure D5): the removed span contributes only those bytes.
/// Remapped offsets are non-decreasing and within the output. An offset at or
/// past `bytes.len()` maps to `out.len()` (the end of the stripped output).
/// These forms start in ground, so the D2 replacement never occurs in them.
pub(in crate::mux) fn strip_rich_content_and_remap(
    bytes: &[u8],
    watch_offsets: &[usize],
) -> (Vec<u8>, Vec<usize>) {
    strip_rich_content_and_remap_with_designator(bytes, watch_offsets, false)
}

/// State-taking form of [`strip_rich_content_and_remap`]: the same single
/// pass, started in the client's "awaiting a charset designator" state when
/// `pending_designator` is true — `bytes[0]` is then the designator of an
/// `ESC (` / `ESC )` that ended the previous feed, so it is copied verbatim
/// (even when it is an `ESC`) and scanning resumes after it. With the flag
/// clear this is exactly [`strip_rich_content_and_remap`].
///
/// Designator transition (mux-suppressed-output-round3-fixes FR2/FR3, the
/// same transition as `term_core`'s escape handling and the write filter's
/// boundary scan): at top level, `ESC (` / `ESC )` and the byte after it are
/// copied verbatim and the pass resumes after them. The third byte is
/// consumed even when it is an `ESC`, so it never starts an OSC, APC, DCS or
/// CSI candidate. `ESC (` / `ESC )` at the very end of the input is copied
/// verbatim; the caller carries the state. Every copied byte, the designator
/// byte included, keeps a one-to-one remapped offset, and the pass stays
/// single and bounded with no backtracking.
pub(in crate::mux) fn strip_rich_content_and_remap_with_designator(
    bytes: &[u8],
    watch_offsets: &[usize],
    pending_designator: bool,
) -> (Vec<u8>, Vec<usize>) {
    let start = if pending_designator {
        WrittenState::Designator
    } else {
        WrittenState::Ground
    };
    let (out, remapped, _) = strip_pass(bytes, watch_offsets, pending_designator, start);
    (out, remapped)
}

/// The single strip pass behind every strip form. `pending_designator` makes
/// the first byte a pending charset designator (copied verbatim) and nothing
/// else; `start` is the end state of the written stream before `bytes`, which
/// only the reported state follows and no removal decision reads. Returns the
/// stripped bytes, the remapped watch offsets and the state of the stream the
/// stripped bytes form (see [`Written`]).
fn strip_pass(
    bytes: &[u8],
    watch_offsets: &[usize],
    pending_designator: bool,
    start: WrittenState,
) -> (Vec<u8>, Vec<usize>, WrittenState) {
    let mut out = Written {
        bytes: Vec::with_capacity(bytes.len()),
        state: start,
    };
    let mut remapped = vec![0usize; watch_offsets.len()];
    let mut next_watch = 0usize;
    let mut i = 0;
    let n = bytes.len();
    // Number of upcoming bytes to copy without examining them: the
    // designator awaited by the caller's flag, or the brace and the
    // designator of an `ESC (` / `ESC )` met at top level.
    let mut verbatim = usize::from(pending_designator);
    // Smallest index at or after which an `ESC \` (ST) terminator may still
    // exist. Once a terminator search runs off the end we set this to `n`, so
    // subsequent APC/DCS introducers short-circuit instead of re-scanning the
    // tail — that is what keeps the whole pass O(n).
    let mut st_search_from = 0usize;
    while i < n {
        // Any watch offset at or before the CURRENT input position maps to
        // the CURRENT output length. Checked at every iteration (including
        // right after a strip jumps `i` past several input bytes at once),
        // so an offset inside a stripped span is caught on the very next
        // iteration with the correct (unchanged, since nothing was pushed
        // for that span) output length.
        while next_watch < watch_offsets.len() && watch_offsets[next_watch] <= i {
            remapped[next_watch] = out.bytes.len();
            next_watch += 1;
        }
        // Designator bytes are copied one at a time so each keeps its own
        // one-to-one remapped offset (the watch loop above runs per byte).
        if verbatim > 0 {
            verbatim -= 1;
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        // Only sequences introduced by ESC are candidates for removal.
        if bytes[i] != 0x1b || i + 1 >= n {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        match bytes[i + 1] {
            b'_' => {
                // APC: ESC _ ... ESC \  — remove only Kitty graphics (ESC _ G).
                if i + 2 < n && bytes[i + 2] == b'G' {
                    if let Some(end) = find_st_terminator(bytes, i + 2, &mut st_search_from) {
                        out.close_before_removal();
                        i = end; // consume through the ST terminator
                        continue;
                    }
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'P' => {
                // DCS: ESC P ... ESC \ — remove only SIXEL.
                if let Some(end) = find_st_terminator(bytes, i + 2, &mut st_search_from) {
                    if dcs_is_sixel(&bytes[i + 2..end - 2]) {
                        out.close_before_removal();
                        i = end;
                        continue;
                    }
                }
                out.push(bytes[i]);
                i += 1;
            }
            b']' => {
                // OSC: ESC ] ... (BEL | ESC \).
                if let Some(end) = find_osc_terminator(bytes, i + 2) {
                    let body = &bytes[i + 2..osc_body_end(bytes, end)];
                    if is_replayable_osc_body(body) {
                        out.close_before_removal();
                        i = end;
                        continue;
                    }
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'[' => {
                // CSI: ESC [ ... final byte — remove only device queries
                // term_core answers (see the module doc comment's "Removed"
                // list / `scan_csi_device_query`).
                if let Some(strip) = scan_csi_device_query(bytes, i + 2) {
                    out.close_before_removal();
                    out.extend(&strip.embedded_c0);
                    i = strip.end;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'(' | b')' => {
                // Charset designation: `ESC`, the brace and the designator
                // byte after it are copied verbatim; the designator is never
                // examined as a fresh introducer.
                out.push(bytes[i]);
                i += 1;
                verbatim = 2;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    // Any remaining watch offsets (including one exactly at `bytes.len()`,
    // which the loop above never revisits since it exits at `i == n`) map
    // to the final output length.
    while next_watch < watch_offsets.len() {
        remapped[next_watch] = out.bytes.len();
        next_watch += 1;
    }
    (out.bytes, remapped, out.state)
}

/// Find the index just past an ST terminator (`ESC \`) for a sequence whose
/// body starts at `from`. Returns the index of the byte AFTER the trailing
/// `\\`, or `None` if no ST terminator is present.
///
/// `st_search_from` caches the smallest index at or after which an ST
/// terminator may still exist (monotonically non-decreasing). When a search
/// runs off the end, `st_search_from` is bumped to `bytes.len()` so a later
/// introducer never re-scans the same terminator-free tail — collapsing what
/// would otherwise be repeated O(n) scans (one per unterminated introducer)
/// into a single O(n) sweep.
fn find_st_terminator(bytes: &[u8], from: usize, st_search_from: &mut usize) -> Option<usize> {
    // Start the scan no earlier than the introducer body and no earlier than
    // the last position we know still might hold a terminator.
    let mut j = from.max(*st_search_from);
    while j + 1 < bytes.len() {
        if bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
            return Some(j + 2);
        }
        j += 1;
    }
    // No ST terminator from `j` to the end — record that there is none at or
    // after `from` so future introducers short-circuit.
    *st_search_from = bytes.len();
    None
}

/// Find the index just past an OSC terminator (BEL `0x07` or ST `ESC \`) for an
/// OSC whose body starts at `from`. Returns the index of the byte AFTER the
/// terminator, or `None` if the OSC is unterminated.
///
/// This scan is inherently bounded: it stops at the first bare ESC that is not
/// the start of ST, so an unterminated OSC introducer only scans its own short
/// run (up to the next ESC), never the whole tail.
fn find_osc_terminator(bytes: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == 0x07 {
            return Some(j + 1);
        }
        if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
            return Some(j + 2);
        }
        // A bare ESC that is not the start of ST aborts the OSC scan.
        if bytes[j] == 0x1b {
            return None;
        }
        j += 1;
    }
    None
}

/// Given `end` (one past the OSC terminator, from `find_osc_terminator`),
/// return the index where the OSC body ends (exclusive of the terminator).
fn osc_body_end(bytes: &[u8], end: usize) -> usize {
    // ST terminator is 2 bytes (ESC \), BEL is 1 byte.
    if end >= 2 && bytes[end - 2] == 0x1b && bytes[end - 1] == b'\\' {
        end - 2
    } else {
        end - 1
    }
}

/// Decide whether a DCS body (the bytes between `ESC P` and the ST terminator)
/// is a SIXEL graphic, which must be stripped from a replay snapshot.
///
/// A SIXEL sequence is `DCS <P1>;<P2>;<P3> q …` — i.e. the DCS final byte (the
/// first byte that is neither a parameter byte `0x30..=0x3B` nor an
/// intermediate byte `0x20..=0x2F`) is `q` (`0x71`). Matching only the final
/// byte avoids mis-classifying a non-SIXEL DCS (e.g. a DECRQSS reply
/// `DCS $ t … ST`) whose *data* merely contains the byte `q`.
fn dcs_is_sixel(body: &[u8]) -> bool {
    let mut k = 0;
    // Skip leading parameter bytes (0x30–0x3B: digits, ':' and ';').
    while k < body.len() && matches!(body[k], 0x30..=0x3b) {
        k += 1;
    }
    // Skip intermediate bytes (0x20–0x2F).
    while k < body.len() && matches!(body[k], 0x20..=0x2f) {
        k += 1;
    }
    // The final byte (first non-param, non-intermediate) decides the DCS kind.
    body.get(k) == Some(&b'q')
}

/// Decide whether an OSC body (the bytes between `ESC ]` and the terminator)
/// is a replayable rich-content launch sequence that must be stripped.
///
/// The body is identified through the shared identification
/// ([`crate::mux::osc_identify`]) — the same number and data the client's
/// parser reconstructs — rather than by matching a byte prefix. The
/// identification borrows the body: it copies nothing, allocates nothing and
/// does not validate the body as UTF-8, because this runs for every OSC the
/// shell emits on the write path (round3 FR8). The strip
/// selection is: a viewer launch of every kind (image included), a Markdown
/// launch, and an agent-status report (SPEC FR4: the OSC report itself is
/// never replayed — the daemon resyncs current state out-of-band after a
/// snapshot). Fold marks, mux control, every other kind and number, and an
/// overflowed number are kept.
///
/// Identical for the write path and the snapshot path (task0004 round-4
/// rework D1' — see [`strip_pty_output_for_scrollback_write`]'s doc
/// comment): there is no more `resize` kind to conditionally strip.
fn is_replayable_osc_body(body: &[u8]) -> bool {
    match osc_body_identity(body) {
        OscIdentity::ViewerLaunch(_)
        | OscIdentity::MarkdownLaunch
        | OscIdentity::AgentStatusReport => true,
        OscIdentity::NotIdentified => false,
    }
}

/// A matched (strippable) CSI device query: where scanning resumes, and any
/// C0 control bytes embedded in the query body that must be re-emitted.
///
/// term_core's parser executes C0 controls encountered mid-CSI immediately
/// without aborting the sequence (`crates/term_core/src/parser/csi.rs`), so
/// dropping them along with the query would change replay behavior —
/// IMPLEMENTATION.md D2.
// mux-snapshot-output-boundary task0001: visibility widened from private to
// `pub(in crate::mux)` so `mux::ipc::pty_spawn::suppressed_output` can reuse
// this predicate (the CSI-device-query SSOT) to decide which CSI queries a
// suppressed chunk must re-deliver (FR9). No behavior change.
pub(in crate::mux) struct CsiStrip {
    /// Index just past the CSI final byte.
    pub(in crate::mux) end: usize,
    /// C0 control bytes (other than ESC) encountered inside the query body,
    /// in order.
    pub(in crate::mux) embedded_c0: Vec<u8>,
}

/// Scan a candidate CSI sequence whose body starts at `from` (the index just
/// past `ESC [`) and decide whether it is a device query that
/// `crates/term_core/src/csi_dispatch.rs` answers with a response — the SSOT
/// this predicate mirrors (see `csi_is_device_query`).
///
/// Body grammar mirrors term_core's actual CSI parser states
/// (`crates/term_core/src/parser/csi.rs`), not a stricter "params then
/// intermediates" grammar:
/// - A private marker (`<=>?`, `0x3C..=0x3F`) is valid ONLY as the very
///   first byte of the body (term_core's `csi_entry` state; embedded C0
///   bytes before it don't count against "first", since `csi_entry`'s C0
///   arm doesn't transition state). Anywhere else it hits term_core's
///   `csi_param` invalid-byte arm and CANCELS the whole CSI — no dispatch,
///   no response — so the scanner returns `None` there too.
/// - Digits and `;`/`:` (`0x30..=0x3B`) keep accumulating into the same
///   first-parameter tracking regardless of whether an intermediate byte
///   has already been seen — term_core's `ParamParser` accumulates digits
///   into `current` independently of the separate `intermediates` vec, so
///   `csi_param`'s digit arm has no "already saw an intermediate" guard.
/// - Intermediate bytes (`0x20..=0x2F`) may appear at any point and keep
///   accumulating (both `csi_entry` and `csi_param` accept them).
/// - A final byte (`0x40..=0x7E`) completes the CSI and dispatches.
///
/// Returns `Some` only for a COMPLETE CSI (a valid final byte is found) that
/// matches the strip predicate. Returns `None` for: a CSI that completes but
/// does not match (the caller preserves it byte-for-byte via its normal
/// single-byte fallback, and the main loop then pushes the rest of the
/// sequence one byte at a time — still an O(n) pass overall), a CSI
/// cancelled by a non-leading private marker (see above), an unterminated
/// CSI (buffer ends before a final byte), a CSI body containing a bare ESC
/// (aborts the candidate — the caller's single-byte fallback naturally
/// re-processes bytes up to that ESC one at a time, so "the scanned prefix
/// is preserved as-is and scanning resumes at that ESC" falls out of the
/// existing fallback without special-casing), or a byte outside the CSI
/// grammar (e.g. DEL).
pub(in crate::mux) fn scan_csi_device_query(bytes: &[u8], from: usize) -> Option<CsiStrip> {
    let mut j = from;
    let mut private_prefix: Option<u8> = None;
    let mut first_param = FirstParam::START;
    let mut param_bytes_seen = 0usize;
    let mut intermediates: Vec<u8> = Vec::new();
    let mut embedded_c0: Vec<u8> = Vec::new();

    loop {
        let b = *bytes.get(j)?; // ran off the end: unterminated CSI
        match b {
            0x1b => return None, // bare ESC aborts the candidate
            0x00..=0x1a | 0x1c..=0x1f => {
                // C0 control other than ESC: does not abort the candidate;
                // recorded for re-emission if the query ends up stripped.
                embedded_c0.push(b);
                j += 1;
            }
            b'<' | b'=' | b'>' | b'?' => {
                // Private marker: valid only as the leading byte of the CSI
                // body (term_core's `csi_entry` state). Once any parameter
                // byte, separator, or intermediate has been seen, a private
                // marker is invalid in `csi_param` and cancels the whole
                // CSI — mirror that by invalidating the candidate.
                if param_bytes_seen == 0 && intermediates.is_empty() {
                    private_prefix = Some(b);
                    param_bytes_seen += 1;
                    j += 1;
                } else {
                    return None;
                }
            }
            0x30..=0x3b => {
                // Digit, ';', or ':'. Accumulates into the first-parameter
                // tracking regardless of intermediates seen so far — mirrors
                // term_core's `ParamParser`, where digits feed `current`
                // independently of the separate intermediates vec. The
                // accumulation is [`FirstParam`], shared with [`OpenCsi`].
                first_param.feed(b);
                param_bytes_seen += 1;
                j += 1;
            }
            0x20..=0x2f => {
                // Intermediate byte — may appear at any point.
                intermediates.push(b);
                j += 1;
            }
            0x40..=0x7e => {
                // Final byte — the CSI is complete.
                return if csi_is_device_query(private_prefix, &intermediates, first_param.value, b)
                {
                    Some(CsiStrip {
                        end: j + 1,
                        embedded_c0,
                    })
                } else {
                    None
                };
            }
            _ => return None, // byte outside the CSI grammar (e.g. DEL)
        }
    }
}

/// The strip predicate (SPEC.md FR1/FR2 "Strip decision" table), mirroring
/// the dispatch conditions in `crates/term_core/src/csi_dispatch.rs`: true
/// only for CSI forms term_core answers with a device response.
///
/// term_core dispatches on `intermediates.first()` only, and truncates the
/// collected intermediates to `MAX_CSI_INTERMEDIATES = 2`
/// (`crates/term_core/src/parser_types.rs`) — so bytes beyond the matched
/// ones never prevent a response. In this filter's variable split, a
/// private marker (`<=>?`) — which can only ever occupy term_core's
/// intermediates slot 0 — is tracked separately as `private_prefix`, so
/// `intermediates` here holds only the 0x20-0x2F bytes that would occupy
/// term_core's remaining slot(s).
///
/// | final | private prefix    | intermediates                           | first param      | query kind |
/// |-------|--------------------|------------------------------------------|-------------------|------------|
/// | `n`   | none               | none                                      | 5 or 6 (mod 256) | DSR / CPR  |
/// | `c`   | none               | none                                      | any               | DA1        |
/// | `c`   | `?` or `>`         | any (trailing bytes ignored)              | any               | DA1 / DA2  |
/// | `t`   | none               | none                                      | 14, 16, 18        | XTWINOPS   |
/// | `p`   | `?`                | first byte `$` (trailing bytes ignored)   | any               | DECRPM     |
///
/// `first_param` treats an empty leading digit run as 0, matching
/// term_core's `ParamParser::get_first_or_zero`.
///
/// The `n` (DSR/CPR) row matches "mod 256" because term_core's dispatch
/// site truncates the parameter to `u8` before comparing —
/// `ParamParser::get_first_or_zero(params) as u8` (csi_dispatch.rs) — after
/// the parameter itself was already clamped to `MAX_PARAM_VALUE = 9999`
/// during accumulation (parser_params.rs). So e.g. `ESC[261n` (261 mod
/// 256 = 5) dispatches DSR and must be stripped. This is the ONLY dispatch
/// site among the ones this predicate mirrors that truncates to `u8`; the
/// DA / XTWINOPS / DECRPM comparisons use the parameter untruncated.
fn csi_is_device_query(
    private_prefix: Option<u8>,
    intermediates: &[u8],
    first_param: u32,
    final_byte: u8,
) -> bool {
    match final_byte {
        b'n' => {
            // Mirror term_core's clamp-then-truncate: clamp to
            // MAX_PARAM_VALUE (9999, matching the accumulation clamp in
            // `scan_csi_device_query`'s saturating accumulator collapsed to
            // the same result) then truncate to u8 before the 5/6 match.
            let truncated = first_param.min(9999) as u8;
            private_prefix.is_none() && intermediates.is_empty() && matches!(truncated, 5 | 6)
        }
        b'c' => match private_prefix {
            None => intermediates.is_empty(),
            Some(b'?') | Some(b'>') => true,
            _ => false,
        },
        b't' => {
            private_prefix.is_none()
                && intermediates.is_empty()
                && matches!(first_param, 14 | 16 | 18)
        }
        b'p' => private_prefix == Some(b'?') && intermediates.first() == Some(&b'$'),
        _ => false,
    }
}

// Visible inside the mux tree so the write filter's tests share the term_core
// end-state oracle defined there (mux-strip-escape-state-carry).
#[cfg(test)]
pub(in crate::mux) mod tests;
