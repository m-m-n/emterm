//! Scrollback write filtering on the reader path: rich-content
//! stripping, alt-screen exclusion, and the agent-status feed scanner.

use std::borrow::Cow;

// The CSI sub-state is defined in the shared strip module (so that module never
// depends on the IPC layer); it stays nameable here under its current name.
pub(in crate::mux) use crate::mux::scrollback_filter::CsiPhase;
use crate::mux::scrollback_filter::strip_pty_output_for_scrollback_write_with_csi_state;
use crate::mux::session::pane::AgentStatusFeedItem;

/// Read PTY output in a blocking loop and forward to the output target.
/// Runs in a dedicated std::thread since PTY reads are blocking I/O.
///
/// When the connected channel fails (GUI disconnected), the reader automatically
/// switches to buffering mode using the per-pane scrollback buffer. The reader
/// thread stays alive so the PTY process output is never lost.
///
/// Phase B: bytes are written into `scrollback` only on the detached arms
/// (matching the previous per-detach-cycle ring buffer behavior). Phase C
/// will move the write above the `output_target` match so attach-time bytes
/// are also retained.
/// Extract the main-buffer byte spans of a raw PTY chunk for the scrollback
/// ring, given `alt_at_start` (the shadow parser's alt-screen state *before*
/// this chunk). The alternate screen has no scrollback, so its output — and
/// the buffer-switch toggles themselves (`?1049` / `?1047` / `?47` `h`/`l`) —
/// are dropped; only main-buffer bytes survive. A chunk that crosses a buffer
/// switch keeps the main-buffer side rather than being discarded wholesale
/// (which would lose e.g. command output emitted just before a TUI opens in
/// the same read).
///
/// Returns `(bytes, final_alt)`. `final_alt` is the alt state the scan ended
/// in; the caller cross-checks it against the authoritative post-chunk parser
/// state to detect a toggle that straddled the read boundary (or an
/// unrecognized form) and fall back conservatively. `Cow::Borrowed` is
/// returned for the common no-toggle chunk (whole chunk on main, empty on
/// alt) so the hot path avoids a copy.
/// Cap on the per-pane pending buffer inside [`ScrollbackWriteFilter`]. When
/// the pending run of bytes we could not yet strip grows past this many bytes,
/// the filter gives up the strip guarantee for that flush and forwards the
/// pending bytes verbatim to the ring. Sized comfortably above one
/// `emterm markdown|json|yaml` chunk (128 KiB payload, ~172 KiB after base64
/// framing) so the common case never trips it.
pub(super) const SCROLLBACK_FILTER_PENDING_CAP: usize = 512 * 1024;

/// Stateful stream filter that strips viewer-launch rich content (OSC 777
/// emterm-{markdown,image,json,yaml} / Kitty APC / SIXEL DCS / OSC 9999
/// emterm-md) — AND a `resize` kind OSC 777 body (review round-1 rework,
/// finding `0c18ff55032328ab`: a forged in-band resize marker must never
/// reach the ring from PTY output) — BEFORE bytes land in the scrollback
/// ring.
///
/// **Why stateful:** PTY reads are chunked at 64 KiB, but an `emterm markdown`
/// / `image` / `json` / `yaml` CLI emits a single OSC 777 chunk of up to
/// 128 KiB payload (~172 KiB after base64 framing). One CLI chunk therefore
/// spans multiple `read()` calls, so a stateless per-chunk stripper sees
/// either (introducer, no terminator) or (terminator, no introducer) and —
/// by design — passes both fragments through verbatim. The fragments then
/// land in the 2 MiB ring, and a later overflow can evict the introducer
/// while its base64 tail survives; the snapshot-time stripper (which only
/// matches complete sequences) then replays that headerless tail into the
/// client's grid on tab-switch reattach.
///
/// This filter closes that gap by holding an unterminated introducer's bytes
/// in `pending` until a subsequent [`Self::feed`] carries the terminator, at
/// which point the fully-formed sequence is stripped in one shot.
/// `pending` is capped at [`SCROLLBACK_FILTER_PENDING_CAP`]; on overflow the
/// pending bytes are forwarded raw (the escape hatch — the ring may then
/// contain a partial sequence, but that is strictly better than an
/// unbounded per-pane buffer).
///
/// Live-forwarded `data.to_vec()` to the connected client is intentionally
/// untouched, so viewer launch on the client side is unaffected. The
/// snapshot-time stripper in `scrollback_filter.rs` remains as a
/// defense-in-depth guard for scrollback captured by an older daemon that
/// predates this filter.
pub(in crate::mux) struct ScrollbackWriteFilter {
    pending: Vec<u8>,
    /// The `(cols, rows)` in effect when the CURRENT `pending` run started
    /// accumulating (i.e. the read whose chunk first left an unterminated
    /// strip-target introducer behind). `None` exactly when `pending` is
    /// empty — task0005 rework D7'' (review round-4 finding
    /// `0e3f8378913e1f4a`). See [`Self::feed`]'s doc for the attribution
    /// rationale.
    pending_started_dims: Option<(u16, u16)>,
    /// Client-parity "awaiting designator" state (mux-suppressed-output-
    /// round2-fixes FR3): the fed stream ended right after `ESC (` / `ESC )`
    /// outside any string, so the client's parser consumes the FIRST byte of
    /// the next feed as the charset designator — even when it is an `ESC`.
    /// Kept apart from `pending`: nothing is held for a designator, so
    /// `pending` is always empty while this is set.
    awaiting_designator: bool,
    /// Client-parity "inside a CSI" state (mux-suppressed-output-round4-fixes
    /// FR4): the CSI sub-state of the stream emitted so far — what a parser
    /// replaying the ring is in after its last byte. Set when the emitted
    /// stream ends right after `ESC [` or inside a CSI's parameter and
    /// intermediate bytes; cleared by the byte that completes or cancels the
    /// CSI, by an `ESC` that is emitted, and by a cut (which closes it, see
    /// [`CSI_CLOSING`]). Nothing is held for a CSI: the bytes are written
    /// as they arrive, and only this O(1) state is carried. It can stand
    /// together with a non-empty `pending`: the held run then starts at an
    /// `ESC`, and the stream emitted before it is still inside the CSI that
    /// `ESC` would abort once it is written (with a held chain, the `ESC`
    /// that heads the chain: the state is captured there, not at the chain's
    /// last construct). It is never set together with `awaiting_designator`.
    ///
    /// mux-cut-csi-post-strip-closure FR2: the state is that of the bytes
    /// actually WRITTEN, after the strip. The strip reports it while it removes
    /// what it removes (D1), so a construct it removes together with its
    /// opening `ESC` does not clear it: the written bytes still end inside the
    /// CSI that `ESC` would have aborted. A call that drains bytes before the
    /// held chain sets it from the stripped output of those bytes, started from
    /// the state carried in; a call that drains nothing leaves it unchanged.
    csi: Option<CsiPhase>,
    /// Chain bookkeeping (mux-suppressed-output-round4-fixes FR1): where the
    /// LAST construct of the held chain starts, as an offset into `pending`.
    /// `pending` holds a whole chain from its head (consecutive constructs
    /// each closed by the `ESC` that opens the next, see
    /// [`ScrollbackWriteFilter::feed_with_cuts`]); the links before the last
    /// construct are settled, so the next scan resumes here instead of
    /// re-walking them. `None` exactly when `pending` is empty; `Some(0)` for
    /// a chain of one construct.
    held_construct_start: Option<usize>,
}

/// The write a cut makes when the emitted stream ends inside a CSI
/// (mux-suppressed-output-round4-fixes FR4, D3): DEL. In both CSI sub-states
/// `term_core` treats it as invalid, cancels the CSI without dispatching it
/// and returns to ground — no character, cursor move or response — and in
/// ground it is ignored. It is not an `ESC`, so neither strip reads it as
/// the start of a strip target, and the CSI scan of the strip treats it as
/// outside the CSI grammar.
pub(in crate::mux) const CSI_CLOSING: &[u8] = &[0x7f];

/// A sequence whose opening `ESC` was held in `pending` when a feed started
/// and that reached its terminator (BEL for OSC, or ST) inside that feed,
/// before any cut (mux-suppressed-output-round2-fixes FR6). Recorded by the
/// boundary scan the feed already runs; carries the scanned run by move, so
/// the non-suppressed reader path pays no copy for it.
pub(in crate::mux) struct CarriedCompletion {
    /// The raw run the boundary scan walked (`old pending ++ fed head`);
    /// only `run[start..end]` is the sequence.
    run: Vec<u8>,
    /// Where the completed construct's opening `ESC` sits within `run`: the
    /// held chain's last construct, not its head (FR1).
    start: usize,
    /// One past the terminator's last byte within `run`.
    end: usize,
    /// Fed offset just past the terminator.
    fed_end: usize,
}

impl CarriedCompletion {
    /// The complete sequence bytes, from its opening `ESC` through its
    /// terminator.
    pub(in crate::mux) fn bytes(&self) -> &[u8] {
        &self.run[self.start..self.end]
    }

    /// Fed offset just past the sequence's terminator.
    pub(in crate::mux) fn fed_end(&self) -> usize {
        self.fed_end
    }
}

/// Result of one [`ScrollbackWriteFilter::feed_with_cuts`] call.
pub(in crate::mux) struct FeedOutcome {
    /// The dims to attribute `bytes` to (see [`ScrollbackWriteFilter::feed`]).
    pub(in crate::mux) dims: (u16, u16),
    /// Bytes safe to write to the scrollback ring right now.
    pub(in crate::mux) bytes: Vec<u8>,
    /// The carried-over completion of THIS call, if any. Never populated
    /// when the call took the overflow flush.
    pub(in crate::mux) carried: Option<CarriedCompletion>,
}

impl ScrollbackWriteFilter {
    pub(in crate::mux) fn new() -> Self {
        Self {
            pending: Vec::new(),
            pending_started_dims: None,
            awaiting_designator: false,
            csi: None,
            held_construct_start: None,
        }
    }

    /// Feed one PTY read chunk, produced under `current_dims`. Returns the
    /// dims to attribute the returned bytes to, together with the bytes
    /// themselves safe to write to the scrollback ring right now. Any
    /// trailing bytes belonging to an unterminated strip-target introducer
    /// are held in `pending` until the next feed.
    ///
    /// **Attribution (task0005 rework D7'', review round-4 finding
    /// `0e3f8378913e1f4a`):** `pending` carries bytes across reads, so the
    /// bytes a `feed` call RETURNS can include a run that was carried over
    /// from an EARLIER read — produced under whatever dims were in effect
    /// THEN, not necessarily `current_dims`. Returning `current_dims`
    /// unconditionally (the pre-fix behavior) misattributes that carried-
    /// over content to the dims of the read that merely happened to flush
    /// it — normally a few bytes, but up to the full
    /// [`SCROLLBACK_FILTER_PENDING_CAP`] (512 KiB) on the overflow escape
    /// hatch below. Instead: when `pending` is EMPTY at the start of this
    /// call, the call's whole output originates from `chunk` itself, so
    /// `current_dims` applies directly (the overwhelmingly common case —
    /// no resize raced an unterminated introducer). When `pending` already
    /// held carried-over bytes, this call's output is attributed to the
    /// dims recorded when THAT run started (`pending_started_dims`) — the
    /// dims in effect for at least the leading portion of what is flushed,
    /// which is a strictly more accurate attribution than blaming the
    /// newest read for content mostly (or entirely) produced earlier.
    ///
    /// Overflow escape hatch: if `pending` (after appending `chunk`) exceeds
    /// [`SCROLLBACK_FILTER_PENDING_CAP`], the entire pending run is flushed
    /// early WITHOUT waiting for a safe boundary. This trades "flush at a
    /// structurally clean boundary" for a bounded per-pane memory footprint
    /// — a wedged / adversarial stream cannot pin arbitrary bytes in the
    /// buffer.
    ///
    /// task0003 D1 (review round-2 finding `a6ab9b340119beed`, critical):
    /// the flushed bytes still go through
    /// [`strip_pty_output_for_scrollback_write_with_designator`] — they are NOT forwarded
    /// raw. Before this fix, the overflow path returned `pending` verbatim,
    /// so a child process could force this branch (an unterminated OSC/DCS/
    /// APC introducer padded past the cap) and have a forged resize marker
    /// anywhere in that padding reach the scrollback ring completely
    /// unfiltered — the cap bounds MEMORY, not the strip guarantee; a
    /// complete marker is removed in a single linear pass regardless of how
    /// this batch was flushed.
    pub(in crate::mux) fn feed(
        &mut self,
        chunk: &[u8],
        current_dims: (u16, u16),
    ) -> ((u16, u16), Vec<u8>) {
        let outcome = self.feed_with_cuts(chunk, current_dims, &[]);
        (outcome.dims, outcome.bytes)
    }

    /// Cut-aware feed (mux-suppressed-output-round2-fixes FR3/FR4/FR6): like
    /// [`Self::feed`], but `cuts` lists — ascending, possibly repeating, each
    /// between 0 and `fed.len()` inclusive — the fed-coordinate positions
    /// where extraction REMOVED a screen-switch sequence (47/1047/1049 `h`/
    /// `l`) from the raw chunk. The client saw that sequence's `ESC`; this
    /// filter did not. At each cut the filter does what the client did at
    /// that `ESC`:
    ///
    /// - an in-progress OSC/DCS/APC string, or a held lone `ESC`, is closed
    ///   and NOT written (round3 FR1): the bytes from its opening `ESC` on are
    ///   absent from the emitted bytes and no terminator is appended, so a
    ///   later BEL / ST in the ring can never complete what the client already
    ///   closed. The same holds for the whole chain it closes (round4 FR1,
    ///   see "Chains" below): the chain is dropped from its head. The settled
    ///   bytes before it are emitted through the strip;
    /// - an `ESC (` / `ESC )` that ends the run, awaiting its designator, is
    ///   emitted whole with the rest (nothing is held for it, as-08) and is
    ///   followed by one ESC (round4 FR3, see "Awaiting designator" below);
    ///   the awaiting-designator flag is cleared;
    ///
    /// and processing continues from ground with the next fed byte. An empty
    /// `fed` with a non-empty `cuts` still closes whatever is held.
    ///
    /// **Chains (mux-suppressed-output-round4-fixes FR1).** An OSC/DCS/APC
    /// string aborted by an `ESC` that opens another construct, and the first
    /// `ESC` of an `ESC ESC`, are closed by the `ESC` that follows them; they
    /// are links, consecutive constructs each closed by the next one's
    /// opening `ESC`. Those `ESC`s are the client's, not settled sequences:
    /// a cut after the chain's last construct opens aborts the construct the
    /// client is in, and a later BEL / ST must not complete any link of it
    /// on replay. So `pending` holds the whole chain from its head, not only
    /// its last construct. A chain ends — everything from its head is
    /// settled and drains through the strip — on a completed string, on a
    /// plain byte in ground, on a complete non-string escape (`ESC [`, a
    /// two-byte dispatch) and on `ESC (` / `ESC )`. The scan of a carried
    /// run resumes at the stored start of the chain's last construct
    /// instead of re-walking the settled links, so a chain of any length
    /// costs one pass over its bytes.
    ///
    /// **Postcondition.** After every call, `pending` holds only the single
    /// chain still open at the end of the fed stream after its last cut,
    /// from its head, ending in the incomplete string — or lone `ESC` — it
    /// is waiting on; never a construct closed by a cut, nor any byte after
    /// one. (A chain closed by a cut is not written either: it leaves
    /// neither `pending` nor the emitted bytes.)
    ///
    /// **Carried-over completion.** When the sequence whose opening `ESC`
    /// was held in `pending` at the start of the call — the last construct of
    /// the held chain — reaches its terminator inside the call, before any
    /// cut, the outcome reports it (recorded by the boundary scan itself — no
    /// extra pass): the report's bytes are that construct's, from its own
    /// `ESC`, not the links before it. Nothing is reported when that
    /// sequence is aborted, closed by a cut, or still incomplete, nor when
    /// the call took the overflow flush. A completion always ends before the
    /// construct a cut closes, so its bytes stay readable from the run the
    /// report keeps.
    ///
    /// **Awaiting designator.** Every strip call receives, as its initial
    /// flag, whether the run it strips starts with a pending designator byte
    /// (the wait carried in with an empty `pending`), so a designator `ESC`
    /// never starts a strip target there either.
    ///
    /// **Awaiting designator at a cut (round4 FR3).** The client is awaiting a
    /// designator at a cut when the run before the cut ends in `ESC (` /
    /// `ESC )` outside any string, or when the wait was carried in with an
    /// empty `pending` and nothing in this call has consumed the designator
    /// byte yet. The cut's `ESC` was consumed by the client as that
    /// designator, so the filter writes exactly one `ESC` (0x1B) after the
    /// bytes it emits for the run and clears the flag; replay then takes that
    /// byte as the designator and the bytes after the cut start from ground.
    /// The same rule holds on every path: a cut inside the call, a cut at an
    /// empty segment (an earlier read left the wait with an empty `pending`;
    /// the fed range is empty or not), and a cut that follows an overflow
    /// flush in its segment (the end-of-run wait is computed for the flushed
    /// run on that path only). The reader's fallback closing, an empty fed
    /// range with a cut at 0, reaches the empty-segment path. The `ESC` is
    /// written after the strip, so it never starts a strip target, and it is
    /// part of the bytes attributed to the outcome's dims. Nothing is written
    /// when no designator is awaited: after a complete designation, after a
    /// designator consumed by the segment's first byte, or after a run that
    /// ends in an open string (which the cut drops).
    ///
    /// **CSI at a cut (mux-suppressed-output-round4-fixes FR4).** The client
    /// aborted any CSI it was inside at the removed switch's `ESC`. When the
    /// emitted stream ends inside a CSI at a cut — the CSI opened in this
    /// call, or carried in from an earlier one, with a dropped construct or
    /// chain after it or none — one [`CSI_CLOSING`] write follows the emitted
    /// bytes, on every path the designator `ESC` above is written on: a cut
    /// inside the call, a cut at an empty segment (the reader's fallback
    /// closing feeds an empty range with a cut at 0) and after an overflow
    /// flush that a cut follows. Nothing is written when the CSI was completed or
    /// cancelled before the cut, and never together with the designator `ESC`
    /// (an `ESC` that opens the designator wait aborts the CSI first). A CSI
    /// is never held: only its O(1) sub-state is carried to the next feed,
    /// and the cut clears it. The state is that of the emitted stream at the
    /// head of the dropped chain: with `ESC[6 ESC ESC` the superseded first
    /// `ESC` is a link of the chain, dropped with it, so the stream ends
    /// inside the CSI and the cut closes it (round4 FR1 with FR4).
    ///
    /// **Post-strip state (mux-cut-csi-post-strip-closure FR1-FR3).** "The
    /// emitted stream" is the bytes the filter actually WRITES, after the
    /// strip, not the bytes it was fed. An `ESC` the strip removes together
    /// with its construct (an OSC 777 viewer launch, OSC 9999 emterm-md, an
    /// agent-status report, a Kitty APC, a SIXEL DCS, an answered CSI device
    /// query) does not abort the CSI the written stream is inside, so the cut
    /// closes it and the next feed carries it. The state is reported by the
    /// strip's own pass, started from the state carried in
    /// ([`strip_pty_output_for_scrollback_write_with_csi_state`]); the boundary
    /// scan keeps the boundary, the held chain and the awaiting-designator
    /// state, and decides neither the closing nor the carried CSI state. The
    /// same holds on the overflow flush: the whole flushed run is stripped with
    /// the new form, and in the last segment its reported state is carried.
    ///
    /// **Overflow.** Past [`SCROLLBACK_FILTER_PENDING_CAP`] the run is
    /// flushed exactly as [`Self::feed`] documents, whole chain included;
    /// afterwards `pending` is empty, no chain is held and the
    /// awaiting-designator flag equals the client-parity state at the end of
    /// the flushed run (one bounded pass, overflow path only).
    pub(in crate::mux) fn feed_with_cuts(
        &mut self,
        fed: &[u8],
        current_dims: (u16, u16),
        cuts: &[usize],
    ) -> FeedOutcome {
        if fed.is_empty() && cuts.is_empty() {
            return FeedOutcome {
                dims: current_dims,
                bytes: Vec::new(),
                carried: None,
            };
        }
        let had_carry_over = !self.pending.is_empty();
        let attribution_dims = if had_carry_over {
            self.pending_started_dims.unwrap_or(current_dims)
        } else {
            // Fresh start: remember these dims in case `fed` itself
            // leaves an unterminated introducer pending past this call.
            self.pending_started_dims = Some(current_dims);
            current_dims
        };

        let mut out: Vec<u8> = Vec::new();
        let mut carried: Option<CarriedCompletion> = None;
        let mut overflowed = false;
        // True while `pending` still holds exactly the run this call started
        // with (nothing drained, closed or flushed).
        let mut pending_untouched = true;
        let mut seg_start = 0usize;
        let segment_count = cuts.len() + 1;
        for seg_no in 0..segment_count {
            let is_last = seg_no + 1 == segment_count;
            let seg_end = if is_last {
                fed.len()
            } else {
                cuts[seg_no].min(fed.len()).max(seg_start)
            };
            let seg = &fed[seg_start..seg_end];
            seg_start = seg_end;

            let carry_len_before = self.pending.len();
            self.pending.extend_from_slice(seg);
            if self.pending.is_empty() {
                // Nothing held and nothing fed in this segment; a cut still
                // ends the client's designator wait. The wait can only have
                // been carried in with an empty `pending` and nothing has
                // consumed the designator byte yet, so the cut's ESC is the
                // designator: write it (round4 FR3).
                if !is_last && self.awaiting_designator {
                    out.push(0x1b);
                    self.awaiting_designator = false;
                }
                // A cut also closes a CSI the emitted stream was left inside
                // (FR4).
                if !is_last && self.csi.take().is_some() {
                    out.extend_from_slice(CSI_CLOSING);
                }
                continue;
            }
            let carried_run = carry_len_before > 0;
            // The first byte after `ESC (` / `ESC )` is the designator,
            // consumed unconditionally — it can only be the first byte of
            // the run, because the flag implies an empty `pending`.
            let skip = if !carried_run && self.awaiting_designator {
                1
            } else {
                0
            };
            // Where the boundary scan starts. A held chain's links before its
            // last construct are settled (closed by the `ESC` that opened the
            // next one), so a carried run resumes at that construct's `ESC`
            // (FR1); any other run starts at its first byte, or right after
            // the designator it awaits.
            let scan_start = if carried_run {
                self.held_construct_start.unwrap_or(0)
            } else {
                skip
            };

            if self.pending.len() > SCROLLBACK_FILTER_PENDING_CAP {
                log::warn!(
                    "scrollback write filter: pending exceeded {} bytes, flushing early",
                    SCROLLBACK_FILTER_PENDING_CAP
                );
                overflowed = true;
                pending_untouched = false;
                self.held_construct_start = None;
                let run = std::mem::take(&mut self.pending);
                // Client-parity wait at the end of the flushed run (one
                // bounded pass, overflow path only). In the last segment it
                // is carried as the new flag; before a cut it decides the
                // closing write instead (round4 FR3).
                let awaiting_at_end =
                    scan_boundary(&run, scan_start, carried_run).awaiting_designator;
                // The CSI state comes from the strip of the whole run,
                // started from the state carried in (post-strip closure FR3):
                // a run that ends in an incomplete construct is flushed whole,
                // so its end is inside that construct, not inside a CSI.
                let (stripped, csi_at_end) =
                    strip_pty_output_for_scrollback_write_with_csi_state(&run, skip == 1, self.csi);
                out.extend_from_slice(&stripped);
                if is_last {
                    self.awaiting_designator = awaiting_at_end;
                    self.csi = csi_at_end;
                } else {
                    self.awaiting_designator = false;
                    self.csi = None;
                    // At most one closing write per cut, and never the
                    // designator ESC together with the DEL (EC-6).
                    if awaiting_at_end {
                        out.push(0x1b);
                    } else if csi_at_end.is_some() {
                        out.extend_from_slice(CSI_CLOSING);
                    }
                }
                continue;
            }

            let scan = scan_boundary(&self.pending, scan_start, carried_run);
            if !is_last {
                // A cut follows: the client's ESC closes whatever is open.
                // The single incomplete construct at the end of the run (an
                // OSC / DCS / APC string, or a held lone ESC, from its
                // opening ESC on) is dropped: the bytes before it are
                // emitted through the strip and nothing is held or
                // terminated (round3 FR1). A run ending in an awaiting
                // `ESC (` / `ESC )` has no such construct, so it is emitted
                // whole (round2 as-08), followed by one closing ESC (round4
                // FR3).
                pending_untouched = false;
                self.awaiting_designator = false;
                self.held_construct_start = None;
                let mut run = std::mem::take(&mut self.pending);
                // A completed carried-over sequence ends at or before the
                // boundary, so it stays readable from `run[start..end]`.
                run.truncate(scan.boundary);
                // The strip reports the CSI state of the bytes it writes,
                // started from the state carried in (post-strip closure FR1).
                let (stripped, csi_at_cut) =
                    strip_pty_output_for_scrollback_write_with_csi_state(&run, skip == 1, self.csi);
                out.extend_from_slice(&stripped);
                if carried.is_none() {
                    if let Some(end) = scan.carried_end {
                        carried = Some(CarriedCompletion {
                            start: scan_start,
                            end,
                            fed_end: end.saturating_sub(carry_len_before),
                            run,
                        });
                    }
                }
                // A run ending in `ESC (` / `ESC )` is awaiting its
                // designator: the cut's ESC was consumed as that designator,
                // so write one ESC after the emitted bytes (round4 FR3).
                if scan.awaiting_designator {
                    out.push(0x1b);
                }
                // The written stream ends inside a CSI: the client's parser
                // aborted it at the switch's ESC, so close it for a replay
                // of the ring (round4 FR4). The state is that of the bytes
                // written after the strip, so this also holds when the
                // dropped construct starts right after the CSI's bytes and
                // when the strip removed a construct whose ESC would have
                // aborted it (post-strip closure FR1). Never together with
                // the designator ESC above (the escape that opens the wait
                // aborts the CSI first).
                if !scan.awaiting_designator && csi_at_cut.is_some() {
                    out.extend_from_slice(CSI_CLOSING);
                }
                self.csi = None;
                continue;
            }

            self.awaiting_designator = scan.awaiting_designator;
            // The held chain's last construct, re-based to the pending that
            // remains once the settled bytes before the chain head drain.
            self.held_construct_start =
                (scan.boundary < self.pending.len()).then(|| scan.construct_start - scan.boundary);
            if scan.boundary == 0 {
                // Nothing drains: the carried CSI state is unchanged (the
                // held run starts at an `ESC`, which would abort it only once
                // written).
                continue;
            }
            pending_untouched = false;
            let strippable: Vec<u8> = self.pending.drain(..scan.boundary).collect();
            // The carried state becomes the state of the stripped output of
            // the drained bytes, started from the state carried in: a held
            // strip target that completes here and strips to nothing keeps the
            // CSI carried in open (post-strip closure FR2, EC-1).
            let (stripped, csi_after) = strip_pty_output_for_scrollback_write_with_csi_state(
                &strippable,
                skip == 1,
                self.csi,
            );
            out.extend_from_slice(&stripped);
            self.csi = csi_after;
            if let Some(end) = scan.carried_end {
                carried = Some(CarriedCompletion {
                    start: scan_start,
                    end,
                    fed_end: end.saturating_sub(carry_len_before),
                    run: strippable,
                });
            }
        }

        if self.pending.is_empty() {
            self.pending_started_dims = None;
        } else if !(had_carry_over && pending_untouched) {
            // D7''' (round-6 rework, review round-5 finding
            // `fd379025e1900e9f`): a PARTIAL drain (some bytes drained,
            // some retained) — or a cut that closed the earlier run — leaves
            // a tail that is definitely part of THIS call's own `fed`
            // bytes: an unterminated introducer this read's own bytes left
            // behind, not the earlier run `pending_started_dims` still
            // names. Leaving it unchanged (the pre-fix behavior) meant a
            // LATER flush of that tail attributed it to whichever dims
            // started the OLDEST still-pending run, even after multiple
            // reads' worth of content had flowed through in between — the
            // exact misattribution round-4's fix (this same field) closed
            // for the full-drain case. Update it to `current_dims` so the
            // retained tail is attributed to the read that actually
            // produced it.
            self.pending_started_dims = Some(current_dims);
        }
        FeedOutcome {
            dims: attribution_dims,
            bytes: out,
            carried: if overflowed { None } else { carried },
        }
    }

    /// Test / diagnostic: whether the client would consume the next fed
    /// byte as a charset designator (see the field's doc).
    #[cfg(test)]
    pub(in crate::mux) fn awaiting_designator(&self) -> bool {
        self.awaiting_designator
    }

    /// Test / diagnostic: the CSI sub-state the emitted stream is in (see the
    /// field's doc).
    #[cfg(test)]
    pub(in crate::mux) fn csi_phase(&self) -> Option<CsiPhase> {
        self.csi
    }

    /// Where the last construct of the held chain starts within `pending`
    /// (test / diagnostic; see the field's doc).
    #[cfg(test)]
    pub(in crate::mux) fn held_construct_start(&self) -> Option<usize> {
        self.held_construct_start
    }

    /// Number of bytes currently held in `pending` (test / diagnostic).
    #[cfg(test)]
    pub(in crate::mux) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// The bytes currently held in `pending` — an unterminated strip-target
    /// run this filter has not yet been able to classify (mux-snapshot-output-boundary
    /// task0001, FR10 "T" tail). Read-only: `pending` is still owned and
    /// mutated only by [`Self::feed`]. Used by
    /// `mux::ipc::pty_spawn::suppressed_output`'s replacement-payload builder
    /// to re-deliver this filter's own held-back tail when a suppressed
    /// chunk's incomplete trailing run coincides with it.
    ///
    /// Postcondition (mux-suppressed-output-fixes task0002, widened to chains
    /// by mux-suppressed-output-round4-fixes FR1): after every [`Self::feed`]
    /// call, this is either empty, or holds the whole chain that is still
    /// OPEN at the end of the fed main-buffer stream, from its head: the
    /// consecutive constructs each closed by the `ESC` that opens the next
    /// (an OSC/DCS/APC string aborted by an `ESC` that opens another
    /// construct, a superseded first `ESC` of `ESC ESC`) up to the last one,
    /// an OSC/DCS/APC string — or a lone trailing `ESC` — still INCOMPLETE
    /// (see [`scan_boundary`]'s doc for incomplete vs aborted). A chain of
    /// one construct is the single sequence of the earlier contract. It
    /// never holds a settled sequence (complete, or closed by something that
    /// is not the opening of another construct), and never any byte after
    /// the chain. The only exception is right after the
    /// [`SCROLLBACK_FILTER_PENDING_CAP`] overflow flush, when it is always
    /// empty.
    pub(in crate::mux) fn pending(&self) -> &[u8] {
        &self.pending
    }
}

/// Find the head of the still-OPEN chain at the end of `bytes`, scanning from
/// `start`: the position where the consecutive constructs leading to the
/// last still-INCOMPLETE strip-target introducer (or lone trailing ESC)
/// begin. If every strip-target sequence in `bytes` is either closed or
/// genuinely absent, the boundary is `bytes.len()` — everything is safe to
/// emit.
///
/// `start` is 1 exactly when the client is awaiting a charset designator:
/// `bytes[0]` is then that designator, consumed unconditionally (even when
/// it is an `ESC`), and scanning resumes right after it (FR3). For a carried
/// run it is instead the start of the held chain's last construct (the links
/// before it were settled by an earlier scan and need no second walk, round4
/// FR1).
///
/// `carried_candidate` is true when `bytes` begins with bytes held in
/// `pending` from an earlier read — a chain headed at index 0: the construct
/// that opens at `start` is then a carried-over sequence, and when it
/// completes the scan records the index just past its terminator
/// ([`BoundaryScan::carried_end`]) — the FR6 report, made inside this single
/// pass.
///
/// task0002 (mux-suppressed-output-fixes, FR1/FR2/FR5): this scan follows
/// the SAME transition rules `term_core`'s parser applies (see
/// `crates/term_core/src/parser/{osc,dcs,apc,escape}.rs`), so this filter's
/// notion of "where a string is still open" agrees with the client's —
/// see [`ScrollbackWriteFilter::pending`]'s doc for the postcondition this
/// guarantees.
///
/// **Incomplete vs aborted.** An OSC/DCS/APC string opened by `ESC ] / ESC P
/// / ESC _` closes in exactly one of three ways once its introducer is seen:
/// - **Complete**: BEL (OSC only) or `ESC \` (ST, all three kinds). The scan
///   resumes right after the terminator.
/// - **Aborted**: `ESC` followed by any OTHER byte. The string is CLOSED
///   there and that following byte is processed as the start of a fresh
///   escape sequence (mirrors `term_core`'s `*_escape` handlers, which
///   dispatch the string as `Unterminated` and re-feed the byte to
///   `escape()`). The scan resumes AT the aborting `ESC`, not past it, so a
///   string beginning there is recognized as its own attempt. The aborting
///   `ESC` is itself the opening of the next construct, so the closed string
///   is a LINK of a chain (below), not a settled sequence: it is written only
///   once the chain settles.
/// - **Incomplete**: the buffer runs out before either of the above is seen —
///   including an `ESC` that is the very last byte (we don't yet know if the
///   next byte will complete it as `\\`, abort it, or start something else
///   next time). The WHOLE string — from its own opening `ESC`, and from the
///   head of the chain it closes — is held.
///
/// **Chains (round4 FR1).** Consecutive constructs, each closed by the `ESC`
/// that opens the next: an aborted OSC/DCS/APC string whose aborting `ESC`
/// opens another construct, and the superseded first `ESC` of `ESC ESC`. The
/// chain ends at a completed string, a plain byte, a complete non-string
/// escape (`ESC [`, a two-byte dispatch) or `ESC (` / `ESC )`; everything
/// from the head to that point is settled. While it is open, the boundary is
/// the chain's HEAD and [`BoundaryScan::construct_start`] is the start of its
/// last construct. One forward pass: each byte of a chain is visited once,
/// however many links it has.
///
/// **Other escapes** (not OSC/DCS/APC introducers):
/// - `ESC ESC`: the first `ESC` is superseded (mirrors `term_core`'s escape
///   handler staying in the `Escape` state on a second `ESC`); the SECOND
///   `ESC` is the candidate introducer, re-examined on the next loop
///   iteration.
/// - `ESC (` / `ESC )` (charset designation): the byte right after `(`/`)`
///   is ALWAYS consumed as the designator — even if it is itself an `ESC` —
///   and never re-examined as a fresh introducer (mirrors
///   `escape_charset`'s unconditional dispatch). Three bytes are consumed as
///   one unit when all three are present; if the buffer ends right after `(`
///   / `)` with no designator byte yet, nothing is held for it (not an
///   OSC/DCS/APC/lone-ESC shape the pending contract allows), the scan ends,
///   and [`BoundaryScan::awaiting_designator`] is set so the caller consumes
///   the next feed's first byte as the designator (FR3).
/// - A lone `ESC` as the very last byte: held (see "Incomplete" above) —
///   it may still turn out to introduce a string once the next byte arrives.
/// - `ESC [` (CSI, mux-suppressed-output-round4-fixes FR4): nothing is held
///   for a CSI, and the scan steps past `ESC [` as a complete escape. The
///   bytes that follow are skipped like any other byte up to the next `ESC`,
///   which is the only byte that can start something inside a CSI. The scan
///   does NOT decide the CSI state of the emitted stream: whether an `ESC` it
///   steps past is written (and so aborts a CSI) or removed with its construct
///   is the strip's decision, so the strip reports that state
///   (mux-cut-csi-post-strip-closure D1, D3).
/// - Any other `ESC <byte>` (two-byte dispatches like `X` / `^`, etc.): a
///   complete, non-string escape. Not a strip target, not held — the scan
///   just steps past both bytes.
fn scan_boundary(bytes: &[u8], start: usize, carried_candidate: bool) -> BoundaryScan {
    let n = bytes.len();
    let mut i = start.min(n);
    let mut carried_end: Option<usize> = None;
    // The chain being walked (FR1): the index of its head. A run carried in
    // from an earlier read starts inside the chain it holds, headed at index
    // 0; a chain is otherwise opened by the first `ESC` that closes with a
    // following construct.
    let mut chain: Option<usize> = carried_candidate.then_some(0);
    while i < n {
        if bytes[i] != 0x1b {
            chain = None;
            i += 1;
            continue;
        }
        let intro_start = i;
        let carried_here = carried_candidate && intro_start == start;
        // Where a hold from here starts: the chain head, or this `ESC` when
        // no chain is open.
        let head = chain.unwrap_or(intro_start);
        let incomplete = |carried_end| BoundaryScan {
            boundary: head,
            construct_start: intro_start,
            awaiting_designator: false,
            carried_end,
        };
        if i + 1 >= n {
            // Lone trailing ESC: incomplete, held whole (1 byte) together
            // with the chain it closes.
            return incomplete(carried_end);
        }
        match bytes[i + 1] {
            b'_' | b'P' => {
                // APC / DCS. Only Kitty (ESC _ G) is a strip target — but
                // even a non-Kitty APC is still an APC and needs an ESC \
                // terminator before its body is safe to emit; without one,
                // we cannot tell where its body ends. Same tail-buffer rule
                // either way.
                match find_st(bytes, i + 2) {
                    StringScanResult::Complete(end) => {
                        if carried_here {
                            carried_end = Some(end);
                        }
                        i = end;
                        chain = None;
                    }
                    StringScanResult::Aborted(abort_pos) => {
                        // Closed by the `ESC` that opens the next construct:
                        // a link of the chain, not yet settled.
                        i = abort_pos;
                        chain = Some(head);
                    }
                    StringScanResult::Incomplete => return incomplete(carried_end),
                }
            }
            b']' => match find_osc_end(bytes, i + 2) {
                StringScanResult::Complete(end) => {
                    if carried_here {
                        carried_end = Some(end);
                    }
                    i = end;
                    chain = None;
                }
                StringScanResult::Aborted(abort_pos) => {
                    i = abort_pos;
                    chain = Some(head);
                }
                StringScanResult::Incomplete => return incomplete(carried_end),
            },
            0x1b => {
                // ESC ESC: the first ESC is superseded by the second, which
                // is re-evaluated at the next iteration — a link of the
                // chain, not yet settled.
                i += 1;
                chain = Some(head);
            }
            b'(' | b')' => {
                // Charset designation: the next byte is ALWAYS the
                // designator, consumed unconditionally (even if it is an
                // ESC) — never a fresh introducer. If it isn't available yet
                // there is nothing to hold for it, but the client IS now
                // awaiting it: the next feed's first byte is the designator
                // (FR3). The escape is complete, so the chain it closes
                // settles with it.
                if i + 2 < n {
                    i += 3;
                    chain = None;
                } else {
                    return BoundaryScan {
                        boundary: n,
                        construct_start: n,
                        awaiting_designator: true,
                        carried_end,
                    };
                }
            }
            b'[' => {
                // CSI: nothing is held for it. It is a complete escape: it
                // settles the chain it closes, and its bytes are skipped like
                // any others up to the next `ESC`.
                i += 2;
                chain = None;
            }
            _ => {
                i += 2;
                chain = None;
            }
        }
    }
    BoundaryScan {
        boundary: n,
        construct_start: n,
        awaiting_designator: false,
        carried_end,
    }
}

/// Result of [`scan_boundary`].
struct BoundaryScan {
    /// Position of the head of the chain still incomplete at the end of the
    /// buffer (a chain of one construct is headed at its own `ESC`), or the
    /// buffer length when nothing is held. The bytes before it are settled.
    boundary: usize,
    /// Position of the opening `ESC` of that chain's last construct (the
    /// incomplete string or lone `ESC`); `>= boundary`. The buffer length
    /// when nothing is held. A carried run's next scan resumes here.
    construct_start: usize,
    /// The buffer ends right after `ESC (` / `ESC )` outside any string.
    awaiting_designator: bool,
    /// Index just past the terminator of the carried-over sequence that
    /// completed inside this buffer, if any (see `carried_candidate`).
    carried_end: Option<usize>,
}

/// Outcome of scanning for an OSC/DCS/APC string's terminator (see
/// [`find_st`] / [`find_osc_end`]). Named identically in spirit to
/// `mux::ipc::pty_spawn::suppressed_output`'s own `OscScanResult` (that
/// module scans independently — see IMPLEMENTATION.md D1 — this is not a
/// shared type, just the same three-way distinction FR2 requires).
enum StringScanResult {
    /// Index just past the terminator.
    Complete(usize),
    /// Index of the aborting `ESC` (followed by a byte that is not `\\`) —
    /// the string is CLOSED there, not held; the caller resumes scanning AT
    /// this index as a fresh escape sequence.
    Aborted(usize),
    /// The buffer ran out before either a terminator or an abort was seen —
    /// the caller holds the WHOLE string from its own opening `ESC`.
    Incomplete,
}

/// Find how an ST-only string (APC / DCS) starting at `from` (the first body
/// byte, right after the introducer) ends. Mirrors the terminator scan in
/// [`crate::mux::scrollback_filter`] so the boundary detector and the
/// stripper agree on what "complete" means, and `term_core`'s
/// `apc_escape` / `dcs_escape` state handlers for the abort case.
fn find_st(bytes: &[u8], from: usize) -> StringScanResult {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == 0x1b {
            if j + 1 < bytes.len() {
                return if bytes[j + 1] == b'\\' {
                    StringScanResult::Complete(j + 2)
                } else {
                    StringScanResult::Aborted(j)
                };
            }
            // ESC is the last available byte: still incomplete (may yet
            // become ST).
            return StringScanResult::Incomplete;
        }
        j += 1;
    }
    StringScanResult::Incomplete
}

/// Find how an OSC string (terminator BEL or ST) starting at `from` (the
/// first body byte, right after `ESC ]`) ends. Mirrors `term_core`'s
/// `osc_escape` state handler for the abort case.
fn find_osc_end(bytes: &[u8], from: usize) -> StringScanResult {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == 0x07 {
            return StringScanResult::Complete(j + 1);
        }
        if bytes[j] == 0x1b {
            if j + 1 < bytes.len() {
                return if bytes[j + 1] == b'\\' {
                    StringScanResult::Complete(j + 2)
                } else {
                    StringScanResult::Aborted(j)
                };
            }
            return StringScanResult::Incomplete;
        }
        j += 1;
    }
    StringScanResult::Incomplete
}

/// (pattern, is_enter) pairs for the alt-screen toggle CSI sequences.
/// `h` enters the alternate screen, `l` returns to main. Shared with
/// [`AgentStatusFeedScanner`] (below) so its own alt-screen tracking for
/// OSC 133 mark gating (SPEC FR5) stays byte-for-byte consistent with this
/// function's — both must agree on exactly which spans of a chunk count as
/// "live main buffer".
const ALT_SCREEN_TOGGLES: [(&[u8], bool); 6] = [
    (b"\x1b[?1049h", true),
    (b"\x1b[?1049l", false),
    (b"\x1b[?1047h", true),
    (b"\x1b[?1047l", false),
    (b"\x1b[?47h", true),
    (b"\x1b[?47l", false),
];

/// Client-parity state of the byte stream's designator slot (mux-suppressed-
/// output-round4-fixes FR5), carried from one read to the next by the reader
/// (O(1) state; see [`extract_main_buffer`]).
///
/// `term_core` consumes the byte right after `ESC (` / `ESC )` as the charset
/// designator whatever it is, an `ESC` included, so a `ESC [ ? 1049 h` whose
/// `ESC` sits in that slot is the designator plus printed text, not a screen
/// switch. The state records how much of `ESC (` / `ESC )` the previous read
/// ended with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum DesignatorSlot {
    /// The previous read ended with nothing that makes the next byte special.
    #[default]
    Ground,
    /// The previous read ended with a live `ESC` (one `term_core` did not
    /// consume as a designator): a `(` / `)` that opens the next read makes
    /// the byte after it the designator.
    EscapeSeen,
    /// The previous read ended right after `ESC (` / `ESC )`: the first byte
    /// of the next read is the designator.
    Awaiting,
}

/// Result of [`extract_main_buffer`].
pub(super) struct MainBufferExtraction<'a> {
    /// The concatenated main-buffer content of the chunk.
    pub(super) bytes: Cow<'a, [u8]>,
    /// The alt state the scan ended in, as `term_core` has it: a switch whose
    /// `ESC` `term_core` consumes as a designator is not a switch. The reader
    /// reads the spans, which already carry it; the unit tests pin it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) final_alt: bool,
    /// The byte ranges of the chunk that make up `bytes`, in order.
    pub(super) spans: Vec<std::ops::Range<usize>>,
    /// The alt state a parser that treats every `ESC` as an abort reaches at
    /// the chunk's end: each recognized switch counts, a designator-slot one
    /// included. The shadow parser (vt100) is such a parser, so the reader's
    /// cross-check compares THIS state with the shadow's. It equals
    /// `final_alt` unless a switch sat in a designator slot.
    pub(super) shadow_alt: bool,
    /// The designator-slot state at the end of the chunk, for the next read.
    pub(super) slot: DesignatorSlot,
}

/// Convenience form of [`extract_main_buffer`] for a chunk that follows
/// nothing relevant (the designator-slot state starts at ground) when only
/// the main-buffer bytes, the final alt state and the spans are wanted.
///
/// Returns `(bytes, final_alt, spans)`: `bytes` is the concatenated
/// main-buffer content (as before); `spans` are the byte ranges of `data`
/// each contributing to `bytes`, in order — added so a caller can gate
/// OTHER per-byte-position decisions (e.g. [`AgentStatusFeedScanner`]'s OSC
/// 133 mark eligibility) against the exact same main-buffer spans without
/// re-deriving them.
#[cfg(test)]
pub(super) fn extract_main_buffer_bytes(
    data: &[u8],
    alt_at_start: bool,
) -> (Cow<'_, [u8]>, bool, Vec<std::ops::Range<usize>>) {
    let extraction = extract_main_buffer(data, alt_at_start, DesignatorSlot::Ground);
    (extraction.bytes, extraction.final_alt, extraction.spans)
}

/// What the main-buffer scan sees at a toggle position, if it is one.
fn toggle_at(d: &[u8]) -> Option<(&'static [u8], bool)> {
    ALT_SCREEN_TOGGLES
        .iter()
        .find(|(p, _)| d.starts_with(p))
        .copied()
}

/// One forward step of the scan over the designator slot, shared by the fast
/// scan and the span-building scan so both walk the stream identically.
///
/// `i` is the position of the next unexamined byte and `slot` the state the
/// scan is in there. Returns the next live switch — the position of its
/// `ESC`, its pattern and whether it enters the alternate screen — or `None`
/// when the chunk ends first. A switch whose `ESC` is a designator is skipped
/// as `term_core` skips it; `shadow_alt` still follows it (see
/// [`MainBufferExtraction::shadow_alt`]). On a returned switch `i` is left AT
/// its `ESC`, and `slot` is `Ground`.
fn next_live_toggle(
    data: &[u8],
    i: &mut usize,
    slot: &mut DesignatorSlot,
    shadow_alt: &mut bool,
) -> Option<(usize, &'static [u8], bool)> {
    let n = data.len();

    // The designator slot at the chunk's start (carried from the last read).
    // At most two bytes are decided here; everything after is the plain loop.
    if *i == 0 && n > 0 {
        let designator_at = match *slot {
            DesignatorSlot::Awaiting => Some(0),
            DesignatorSlot::EscapeSeen if matches!(data[0], b'(' | b')') => {
                if n > 1 {
                    Some(1)
                } else {
                    *slot = DesignatorSlot::Awaiting;
                    *i = 1;
                    None
                }
            }
            _ => None,
        };
        match designator_at {
            Some(at) => {
                if data[at] == 0x1b {
                    if let Some((_, is_enter)) = toggle_at(&data[at..]) {
                        *shadow_alt = is_enter;
                    }
                }
                *slot = DesignatorSlot::Ground;
                *i = at + 1;
            }
            None => {
                if *slot != DesignatorSlot::Awaiting {
                    *slot = DesignatorSlot::Ground;
                }
            }
        }
    }

    while *i < n {
        if data[*i] != 0x1b {
            *i += 1;
            continue;
        }
        if let Some((pat, is_enter)) = toggle_at(&data[*i..]) {
            *slot = DesignatorSlot::Ground;
            return Some((*i, pat, is_enter));
        }
        // A live `ESC` that opens no switch.
        if *i + 1 >= n {
            *slot = DesignatorSlot::EscapeSeen;
            *i = n;
            break;
        }
        if matches!(data[*i + 1], b'(' | b')') {
            // `ESC (` / `ESC )`: the next byte is the designator, consumed
            // whatever it is, and never starts a switch.
            if *i + 2 < n {
                if data[*i + 2] == 0x1b {
                    if let Some((_, is_enter)) = toggle_at(&data[*i + 2..]) {
                        *shadow_alt = is_enter;
                    }
                }
                *i += 3;
            } else {
                *slot = DesignatorSlot::Awaiting;
                *i = n;
                break;
            }
        } else {
            *i += 1;
        }
    }
    None
}

/// Extract the main-buffer byte spans of a raw PTY chunk for the scrollback
/// ring, given `alt_at_start` (the shadow parser's alt-screen state *before*
/// this chunk) and `slot_at_start` (the designator-slot state the previous
/// read ended in). The alternate screen has no scrollback, so its output —
/// and the buffer-switch toggles themselves (`?1049` / `?1047` / `?47`
/// `h`/`l`) — are dropped; only main-buffer bytes survive. A chunk that
/// crosses a buffer switch keeps the main-buffer side rather than being
/// discarded wholesale (which would lose e.g. command output emitted just
/// before a TUI opens in the same read).
///
/// A switch is only removed when `term_core` treats it as one. The byte
/// right after `ESC (` / `ESC )` is the charset designator, consumed
/// whatever it is, so a switch sequence whose `ESC` sits there is the
/// designator plus printed text: it stays in the output (FR5). The scan
/// follows that rule inside its existing pass over the chunk, with the
/// O(1) [`DesignatorSlot`] carried across reads; a chunk without any switch
/// pattern is still returned borrowed.
///
/// `final_alt` is the alt state the scan ended in. The caller cross-checks
/// [`MainBufferExtraction::shadow_alt`] against the authoritative post-chunk
/// parser state to detect a toggle that straddled the read boundary (or an
/// unrecognized form) and fall back conservatively. `Cow::Borrowed` is
/// returned for the common no-toggle chunk (whole chunk on main, empty on
/// alt) so the hot path avoids a copy.
///
/// Both [`AgentStatusFeedScanner`]'s OSC 133 mark gating (SPEC FR5) and the
/// cut derivation consume the span list, so they follow the same rule.
pub(super) fn extract_main_buffer<'a>(
    data: &'a [u8],
    alt_at_start: bool,
    slot_at_start: DesignatorSlot,
) -> MainBufferExtraction<'a> {
    let mut slot = slot_at_start;
    let mut shadow_alt = alt_at_start;
    let mut i = 0usize;

    // Fast scan for any live toggle. Most chunks (plain output, even
    // SGR-colored) contain none, so we can borrow without building a
    // filtered copy.
    let Some(first) = next_live_toggle(data, &mut i, &mut slot, &mut shadow_alt) else {
        let (bytes, spans) = if alt_at_start {
            (Cow::Borrowed(&[][..]), Vec::new())
        } else {
            (Cow::Borrowed(data), vec![0..data.len()])
        };
        return MainBufferExtraction {
            bytes,
            final_alt: alt_at_start,
            spans,
            shadow_alt,
            slot,
        };
    };

    // Slow path: split into main-buffer spans, dropping toggles and alt spans.
    let mut out = Vec::with_capacity(data.len());
    let mut spans = Vec::new();
    let mut alt = alt_at_start;
    let mut span_start: Option<usize> = if alt { None } else { Some(0) };
    let mut next = Some(first);
    while let Some((at, pat, is_enter)) = next {
        if let Some(s) = span_start.take() {
            out.extend_from_slice(&data[s..at]);
            spans.push(s..at);
        }
        alt = is_enter;
        shadow_alt = is_enter;
        i = at + pat.len();
        if !alt {
            span_start = Some(i);
        }
        next = next_live_toggle(data, &mut i, &mut slot, &mut shadow_alt);
    }
    if let Some(s) = span_start {
        out.extend_from_slice(&data[s..]);
        spans.push(s..data.len());
    }
    MainBufferExtraction {
        bytes: Cow::Owned(out),
        final_alt: alt,
        spans,
        shadow_alt,
        slot,
    }
}

/// Decode state for [`AgentStatusFeedScanner`] — structurally identical to
/// the (separate) `AgentStatusOscScanner` / `Osc133MarkScanner` state
/// machines in `scrollback_filter.rs`, but unified into ONE pass so an OSC
/// 777 `agent-status` report and an OSC 133 mark from the SAME PTY read are
/// emitted in TRUE relative byte order (task0012/task0003, SPEC FR4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AgentStatusFeedScanState {
    /// No partial OSC sequence in flight.
    Idle,
    /// Just consumed an ESC while `Idle`; waiting to see if the next byte is
    /// `]` (OSC introducer).
    SeenEsc,
    /// Inside `ESC ] <body>`, accumulating body bytes in `body` (the
    /// introducer itself is not stored).
    InsideOsc,
    /// The most recent body byte was ESC, not yet pushed into `body`,
    /// pending disambiguation: `\` completes ST, `]` reopens as a fresh OSC
    /// introducer, anything else aborts the in-flight OSC without emitting.
    InsideOscPendingSt,
}

/// Cap on the carry-over held by [`AgentStatusFeedScanner`] for an in-flight
/// (not-yet-terminated) OSC body — mirrors the caps
/// `AGENT_STATUS_SCANNER_CARRY_OVER_CAP` / `OSC133_SCANNER_CARRY_OVER_CAP`
/// in `scrollback_filter.rs` had on the two scanners this type replaces.
const AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP: usize = 8 * 1024;

/// Per-pane stateful scanner producing [`AgentStatusFeedItem`]s — both OSC
/// 777 `agent-status` reports (SPEC FR1/FR3) and live OSC 133 marks (SPEC
/// FR1/FR4/FR5) — from PTY chunks in TRUE byte order within each chunk
/// (finding: FR4's "single ordered feed" contract; the previous
/// implementation ran an `AgentStatusOscScanner` over the full chunk and an
/// `Osc133MarkScanner` over the chunk's live main-buffer subset
/// independently, then the caller concatenated `reports` then `marks` —
/// which is the chunk's SCAN order per scanner, not the chunk's BYTE
/// order. A `D`→`A` pair appearing BEFORE a `Set` report in the actual
/// stream was still forwarded `Set, D, A`).
///
/// Scanning once, with both bodies recognized in the same pass, makes the
/// forwarded order structurally equal to the byte order — there is no
/// second list to merge, so there is nothing left to get out of order.
///
/// FR5 (live-only, main-screen-only) is preserved exactly: the caller
/// passes `live_spans`, the byte ranges of `chunk` already known to be live
/// main-buffer content (the same spans [`extract_main_buffer_bytes`]
/// computes for the scrollback-write path) — a mark is only emitted when
/// its completing byte falls inside one of those spans. Reports remain
/// unconditional (their validity never depended on screen content).
pub(super) struct AgentStatusFeedScanner {
    state: AgentStatusFeedScanState,
    /// Body bytes accumulated for the in-flight OSC (introducer and
    /// terminator excluded).
    body: Vec<u8>,
    /// True once a single carry-over-overflow warning has fired.
    overflow_warned: bool,
}

impl AgentStatusFeedScanner {
    pub(super) fn new() -> Self {
        Self {
            state: AgentStatusFeedScanState::Idle,
            body: Vec::new(),
            overflow_warned: false,
        }
    }

    /// Feed one PTY read chunk. `live_spans` are the byte ranges of `chunk`
    /// eligible for OSC 133 marks (SPEC FR5); reports are recognized
    /// everywhere in `chunk`. Returns every complete report / mark detected
    /// during this call, in the exact order their terminator appeared in
    /// `chunk`. Any trailing incomplete OSC sequence is retained in `self`
    /// and resumed on the next `feed` call.
    pub(super) fn feed(
        &mut self,
        chunk: &[u8],
        live_spans: &[std::ops::Range<usize>],
    ) -> Vec<AgentStatusFeedItem> {
        let mut out = Vec::new();
        for (idx, &b) in chunk.iter().enumerate() {
            self.step(b, idx, live_spans, &mut out);
            if self.body.len() > AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP {
                if !self.overflow_warned {
                    log::warn!(
                        "agent-status feed scanner: carry-over exceeded {} bytes; dropping in-flight sequence",
                        AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP
                    );
                    self.overflow_warned = true;
                }
                self.reset();
            }
        }
        out
    }

    fn step(
        &mut self,
        b: u8,
        idx: usize,
        live_spans: &[std::ops::Range<usize>],
        out: &mut Vec<AgentStatusFeedItem>,
    ) {
        match self.state {
            AgentStatusFeedScanState::Idle => {
                if b == 0x1b {
                    self.state = AgentStatusFeedScanState::SeenEsc;
                }
            }
            AgentStatusFeedScanState::SeenEsc => match b {
                b']' => {
                    self.body.clear();
                    self.state = AgentStatusFeedScanState::InsideOsc;
                }
                0x1b => {
                    // Consecutive ESC: keep the latest one as the candidate
                    // introducer, stay in SeenEsc.
                }
                _ => {
                    self.state = AgentStatusFeedScanState::Idle;
                }
            },
            AgentStatusFeedScanState::InsideOsc => {
                if b == 0x07 {
                    self.commit(idx, live_spans, out);
                } else if b == 0x1b {
                    self.state = AgentStatusFeedScanState::InsideOscPendingSt;
                } else {
                    self.body.push(b);
                }
            }
            AgentStatusFeedScanState::InsideOscPendingSt => match b {
                b'\\' => {
                    self.commit(idx, live_spans, out);
                }
                b']' => {
                    self.body.clear();
                    self.state = AgentStatusFeedScanState::InsideOsc;
                }
                0x1b => {}
                _ => {
                    self.reset();
                }
            },
        }
    }

    /// Complete the in-flight OSC at chunk position `idx` (the index of the
    /// terminator's final byte): emit a report unconditionally, or a mark
    /// only when `idx` falls inside `live_spans` (FR5), then reset to
    /// `Idle` either way.
    fn commit(
        &mut self,
        idx: usize,
        live_spans: &[std::ops::Range<usize>],
        out: &mut Vec<AgentStatusFeedItem>,
    ) {
        if let Some(rest) = self.body.strip_prefix(b"777;emterm;agent-status;") {
            let mut payload = String::from("emterm;agent-status;");
            payload.push_str(&String::from_utf8_lossy(rest));
            out.push(AgentStatusFeedItem::Report(payload));
        } else if live_spans.iter().any(|r| r.contains(&idx)) {
            if let Some(rest) = self.body.strip_prefix(b"133;") {
                let head = rest.split(|&b| b == b';').next().unwrap_or(rest);
                if head.len() == 1 {
                    if let Some(kind) = crate::prompts::PromptMarkKind::from_byte(head[0]) {
                        out.push(AgentStatusFeedItem::Osc133Mark(kind));
                    }
                }
            }
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.body.clear();
        self.state = AgentStatusFeedScanState::Idle;
    }
}
