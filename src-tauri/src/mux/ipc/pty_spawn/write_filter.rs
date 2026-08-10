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
const SCROLLBACK_FILTER_PENDING_CAP: usize = 512 * 1024;

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
}

impl ScrollbackWriteFilter {
    pub(in crate::mux) fn new() -> Self {
        Self {
            pending: Vec::new(),
            pending_started_dims: None,
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
        if chunk.is_empty() {
            return (current_dims, Vec::new());
        }
        let had_carry_over = !self.pending.is_empty();
        let attribution_dims = if had_carry_over {
            self.pending_started_dims.unwrap_or(current_dims)
        } else {
            // Fresh start: remember these dims in case `chunk` itself
            // leaves an unterminated introducer pending past this call.
            self.pending_started_dims = Some(current_dims);
            current_dims
        };
        self.pending.extend_from_slice(chunk);

        if self.pending.len() > SCROLLBACK_FILTER_PENDING_CAP {
            log::warn!(
                "scrollback write filter: pending exceeded {} bytes, flushing early",
                SCROLLBACK_FILTER_PENDING_CAP
            );
            let pending = std::mem::take(&mut self.pending);
            self.pending_started_dims = None;
            return (
                attribution_dims,
                strip_pty_output_for_scrollback_write(&pending),
            );
        }

        let boundary = find_safe_boundary(&self.pending);
        if boundary == 0 {
            return (attribution_dims, Vec::new());
        }
        let strippable: Vec<u8> = self.pending.drain(..boundary).collect();
        if self.pending.is_empty() {
            self.pending_started_dims = None;
        } else {
            // D7''' (round-6 rework, review round-5 finding
            // `fd379025e1900e9f`): a PARTIAL drain (some bytes drained,
            // some retained) leaves a tail that is definitely part of
            // THIS `feed` call's own `chunk` — an unterminated introducer
            // this read's own bytes left behind, not the earlier run
            // `pending_started_dims` still names. Leaving it unchanged
            // (the pre-fix behavior) meant a LATER flush of that tail
            // attributed it to whichever dims started the OLDEST
            // still-pending run, even after multiple reads' worth of
            // content had flowed through in between — the exact
            // misattribution round-4's fix (this same field) closed for
            // the full-drain case. Update it to `current_dims` so the
            // retained tail is attributed to the read that actually
            // produced it.
            self.pending_started_dims = Some(current_dims);
        }
        (
            attribution_dims,
            strip_pty_output_for_scrollback_write(&strippable),
        )
    }

    /// Number of bytes currently held in `pending` (test / diagnostic).
    #[cfg(test)]
    pub(in crate::mux) fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// Find the position of the first unterminated strip-target introducer in
/// `bytes`. If every strip-target sequence in `bytes` is closed (or there
/// are none), returns `bytes.len()` — everything is safe to emit.
///
/// Strip-target introducers we look for (matches
/// [`strip_replayable_rich_content`]):
/// - `ESC _ G` — Kitty APC. Terminator: `ESC \`.
/// - `ESC P` — any DCS. Terminator: `ESC \`. (Non-SIXEL DCS bodies still ride
///   here so an unterminated DCS is not accidentally split — the strip
///   function will decide whether to drop it once complete.)
/// - `ESC ]` — any OSC. Terminator: BEL (`0x07`) or `ESC \`.
///
/// Any other byte after `ESC` (`[` = CSI, standalone escape, etc.) is not a
/// strip target and does not force a boundary.
fn find_safe_boundary(bytes: &[u8]) -> usize {
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if bytes[i] != 0x1b || i + 1 >= n {
            i += 1;
            continue;
        }
        let intro_start = i;
        match bytes[i + 1] {
            b'_' => {
                // APC. Only Kitty (ESC _ G) is a strip target — but even a
                // non-Kitty APC is still an APC and needs an ESC \ terminator
                // before its body is safe to emit; without one, we cannot tell
                // where its body ends. Same tail-buffer rule either way.
                match find_st(bytes, i + 2) {
                    Some(end) => i = end,
                    None => return intro_start,
                }
            }
            b'P' => match find_st(bytes, i + 2) {
                Some(end) => i = end,
                None => return intro_start,
            },
            b']' => match find_osc_end(bytes, i + 2) {
                Some(end) => i = end,
                None => return intro_start,
            },
            _ => {
                i += 2;
            }
        }
    }
    n
}

/// Find the index just past an ST (`ESC \`) terminator starting at or after
/// `from`. Returns the byte index immediately AFTER the trailing `\\`, or
/// `None` if no ST is present. Mirrors the terminator scan in
/// [`crate::mux::scrollback_filter`] so the boundary detector and the
/// stripper agree on what "complete" means.
fn find_st(bytes: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j + 1 < bytes.len() {
        if bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

/// Find the index just past an OSC terminator (BEL `0x07` or ST `ESC \`)
/// starting at `from`. Returns `None` if the OSC is unterminated. A bare
/// `ESC` that is not the start of ST aborts the scan and returns `None`
/// (mirrors `scrollback_filter::find_osc_terminator`).
fn find_osc_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == 0x07 {
            return Some(j + 1);
        }
        if bytes[j] == 0x1b {
            if j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                return Some(j + 2);
            }
            return None;
        }
        j += 1;
    }
    None
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
