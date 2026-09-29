//! Scrollback write filtering on the reader path: rich-content
//! stripping, alt-screen exclusion, and the agent-status feed scanner.

use std::borrow::Cow;

use crate::mux::scrollback_filter::strip_pty_output_for_scrollback_write;
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
}

/// A sequence whose opening `ESC` was held in `pending` when a feed started
/// and that reached its terminator (BEL for OSC, or ST) inside that feed,
/// before any cut (mux-suppressed-output-round2-fixes FR6). Recorded by the
/// boundary scan the feed already runs; carries the scanned run by move, so
/// the non-suppressed reader path pays no copy for it.
pub(in crate::mux) struct CarriedCompletion {
    /// The raw run the boundary scan walked (`old pending ++ fed head`);
    /// only `run[..end]` is the sequence.
    run: Vec<u8>,
    /// One past the terminator's last byte within `run`.
    end: usize,
    /// Fed offset just past the terminator.
    fed_end: usize,
}

impl CarriedCompletion {
    /// The complete sequence bytes, from its opening `ESC` through its
    /// terminator.
    pub(in crate::mux) fn bytes(&self) -> &[u8] {
        &self.run[..self.end]
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
    /// [`strip_pty_output_for_scrollback_write`] — they are NOT forwarded
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
    ///   and emitted through the existing strip, exactly as an ESC-aborted
    ///   string is today;
    /// - the awaiting-designator flag is cleared (the client consumed the
    ///   switch's `ESC` as the designator, as-08);
    ///
    /// and processing continues from ground with the next fed byte. An empty
    /// `fed` with a non-empty `cuts` still closes whatever is held.
    ///
    /// **Postcondition.** After every call, `pending` holds only the single
    /// incomplete string — or lone `ESC` — at the end of the fed stream
    /// after its last cut; never a sequence closed by a cut, nor any byte
    /// after one.
    ///
    /// **Carried-over completion.** When the sequence whose opening `ESC`
    /// was held in `pending` at the start of the call reaches its terminator
    /// inside the call, before any cut, the outcome reports it (recorded by
    /// the boundary scan itself — no extra pass). Nothing is reported when
    /// that sequence is aborted, closed by a cut, or still incomplete, nor
    /// when the call took the overflow flush.
    ///
    /// **Overflow.** Past [`SCROLLBACK_FILTER_PENDING_CAP`] the run is
    /// flushed exactly as [`Self::feed`] documents; afterwards `pending` is
    /// empty and the awaiting-designator flag equals the client-parity state
    /// at the end of the flushed run (one bounded pass, overflow path only).
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
                // ends the client's designator wait.
                if !is_last {
                    self.awaiting_designator = false;
                }
                continue;
            }
            // The first byte after `ESC (` / `ESC )` is the designator,
            // consumed unconditionally — it can only be the first byte of
            // the run, because the flag implies an empty `pending`.
            let skip = if carry_len_before == 0 && self.awaiting_designator {
                1
            } else {
                0
            };

            if self.pending.len() > SCROLLBACK_FILTER_PENDING_CAP {
                log::warn!(
                    "scrollback write filter: pending exceeded {} bytes, flushing early",
                    SCROLLBACK_FILTER_PENDING_CAP
                );
                overflowed = true;
                pending_untouched = false;
                let run = std::mem::take(&mut self.pending);
                // Client-parity flag at the end of the flushed run; a cut
                // after this segment clears it again anyway.
                self.awaiting_designator = if is_last {
                    scan_boundary(&run, skip, false).awaiting_designator
                } else {
                    false
                };
                out.extend_from_slice(&strip_pty_output_for_scrollback_write(&run));
                continue;
            }

            let scan = scan_boundary(&self.pending, skip, carry_len_before > 0);
            if !is_last {
                // A cut follows: the client's ESC closes whatever is open,
                // so the whole run is emitted and nothing is held.
                pending_untouched = false;
                self.awaiting_designator = false;
                let run = std::mem::take(&mut self.pending);
                out.extend_from_slice(&strip_pty_output_for_scrollback_write(&run));
                if carried.is_none() {
                    if let Some(end) = scan.carried_end {
                        carried = Some(CarriedCompletion {
                            end,
                            fed_end: end.saturating_sub(carry_len_before),
                            run,
                        });
                    }
                }
                continue;
            }

            self.awaiting_designator = scan.awaiting_designator;
            if scan.boundary == 0 {
                continue;
            }
            pending_untouched = false;
            let strippable: Vec<u8> = self.pending.drain(..scan.boundary).collect();
            out.extend_from_slice(&strip_pty_output_for_scrollback_write(&strippable));
            if let Some(end) = scan.carried_end {
                carried = Some(CarriedCompletion {
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
    /// task0002 (mux-suppressed-output-fixes) postcondition: after every
    /// [`Self::feed`] call, this is either empty, or starts at the `ESC`
    /// that opens the single OSC/DCS/APC string — or the lone trailing
    /// `ESC` — still INCOMPLETE (see [`scan_boundary`]'s doc for
    /// incomplete vs aborted) at the end of the fed main-buffer stream, and
    /// holds exactly that sequence's bytes: never a closed (complete or
    /// ESC-aborted) sequence, and never any byte after one. The only
    /// exception is right after the [`SCROLLBACK_FILTER_PENDING_CAP`]
    /// overflow flush, when it is always empty.
    pub(in crate::mux) fn pending(&self) -> &[u8] {
        &self.pending
    }
}

/// Find the position of the first still-INCOMPLETE strip-target introducer
/// (or lone trailing ESC) in `bytes`, scanning from `start`. If every
/// strip-target sequence in `bytes` is either closed or genuinely absent,
/// the boundary is `bytes.len()` — everything is safe to emit.
///
/// `start` is 1 exactly when the client is awaiting a charset designator:
/// `bytes[0]` is then that designator, consumed unconditionally (even when
/// it is an `ESC`), and scanning resumes right after it (FR3).
///
/// `carried_candidate` is true when `bytes` begins with bytes held in
/// `pending` from an earlier read: the construct that opens at index 0 is
/// then a carried-over sequence, and when it completes the scan records the
/// index just past its terminator ([`BoundaryScan::carried_end`]) — the
/// FR6 report, made inside this single pass.
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
///   there — not held — and that following byte is processed as the start of
///   a fresh escape sequence (mirrors `term_core`'s `*_escape` handlers,
///   which dispatch the string as `Unterminated` and re-feed the byte to
///   `escape()`). The scan resumes AT the aborting `ESC`, not past it, so a
///   string beginning there is recognized as its own attempt.
/// - **Incomplete**: the buffer runs out before either of the above is seen —
///   including an `ESC` that is the very last byte (we don't yet know if the
///   next byte will complete it as `\\`, abort it, or start something else
///   next time). The WHOLE string — from its own opening `ESC` — is held.
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
/// - Any other `ESC <byte>` (CSI `[`, two-byte dispatches like `X` / `^`,
///   etc.): a complete, non-string escape. Not a strip target, not held —
///   the scan just steps past both bytes.
fn scan_boundary(bytes: &[u8], start: usize, carried_candidate: bool) -> BoundaryScan {
    let n = bytes.len();
    let mut i = start.min(n);
    let mut carried_end: Option<usize> = None;
    while i < n {
        if bytes[i] != 0x1b {
            i += 1;
            continue;
        }
        let intro_start = i;
        let carried_here = carried_candidate && intro_start == 0;
        let incomplete = |carried_end| BoundaryScan {
            boundary: intro_start,
            awaiting_designator: false,
            carried_end,
        };
        if i + 1 >= n {
            // Lone trailing ESC: incomplete, held whole (1 byte).
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
                    }
                    StringScanResult::Aborted(abort_pos) => i = abort_pos,
                    StringScanResult::Incomplete => return incomplete(carried_end),
                }
            }
            b']' => match find_osc_end(bytes, i + 2) {
                StringScanResult::Complete(end) => {
                    if carried_here {
                        carried_end = Some(end);
                    }
                    i = end;
                }
                StringScanResult::Aborted(abort_pos) => i = abort_pos,
                StringScanResult::Incomplete => return incomplete(carried_end),
            },
            0x1b => {
                // ESC ESC: the first ESC is superseded; re-evaluate starting
                // at the second one.
                i += 1;
            }
            b'(' | b')' => {
                // Charset designation: the next byte is ALWAYS the
                // designator, consumed unconditionally (even if it is an
                // ESC) — never a fresh introducer. If it isn't available yet
                // there is nothing to hold for it, but the client IS now
                // awaiting it: the next feed's first byte is the designator
                // (FR3).
                if i + 2 < n {
                    i += 3;
                } else {
                    return BoundaryScan {
                        boundary: n,
                        awaiting_designator: true,
                        carried_end,
                    };
                }
            }
            _ => {
                i += 2;
            }
        }
    }
    BoundaryScan {
        boundary: n,
        awaiting_designator: false,
        carried_end,
    }
}

/// Result of [`scan_boundary`].
struct BoundaryScan {
    /// Position of the first still-incomplete string / lone ESC, or the
    /// buffer length when nothing is held.
    boundary: usize,
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

/// Returns `(bytes, final_alt, spans)`: `bytes` is the concatenated
/// main-buffer content (as before); `spans` are the byte ranges of `data`
/// each contributing to `bytes`, in order — added so a caller can gate
/// OTHER per-byte-position decisions (e.g. [`AgentStatusFeedScanner`]'s OSC
/// 133 mark eligibility) against the exact same main-buffer spans without
/// re-deriving them.
pub(super) fn extract_main_buffer_bytes(
    data: &[u8],
    alt_at_start: bool,
) -> (Cow<'_, [u8]>, bool, Vec<std::ops::Range<usize>>) {
    let matches_toggle = |d: &[u8]| {
        ALT_SCREEN_TOGGLES
            .iter()
            .find(|(p, _)| d.starts_with(p))
            .copied()
    };

    // Fast scan for any toggle. Most chunks (plain output, even SGR-colored)
    // contain none, so we can borrow without building a filtered copy.
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

    // Slow path: split into main-buffer spans, dropping toggles and alt spans.
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
