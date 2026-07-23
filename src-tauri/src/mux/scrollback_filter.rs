//! Scrollback rich-content filtering shared by the mux IPC reattach path and
//! the session pane resume path.
//!
//! This module is the single home for [`strip_replayable_rich_content`] so the
//! session layer (`mux::session::pane`) does not have to reach into the IPC
//! layer (`mux::ipc::reattach`) for it; both depend on this shared module
//! instead.

use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;

/// The OSC 777 `<kind>` token for agent-status reports (SPEC FR1/FR4).
/// Kept as a named constant so the strip predicate and the extraction scan
/// ([`AgentStatusOscScanner`]) share one literal.
const AGENT_STATUS_OSC_KIND: &str = "agent-status";

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
///   terminated) where `<kind>` is one of [`REPLAYABLE_VIEWER_KINDS`]
///   (`markdown` / `image` / `json` / `yaml`). `<kind> == fold` (fold marks)
///   and any other `<kind>` (status-bar, …) are KEPT.
/// - Kitty graphics APC: `ESC _ G … ESC \`
/// - SIXEL DCS: `ESC P <params> q …  ESC \` (only DCS whose final byte is
///   `q`; a DCS whose *data* merely contains `q`, e.g. DECRQSS, is KEPT).
/// - emterm Markdown OSC 9999: `ESC ] 9999 ; emterm-md ; …` (BEL or ST
///   terminated). `ESC ] 9999 ; emterm-mux ; …` (mux control) is KEPT.
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
/// Runs in a single O(n) pass: once an `ESC \` (ST) terminator search runs off
/// the end of the buffer, that "no more ST terminators" fact is cached in
/// `st_search_from` so later APC / DCS introducers do not re-scan the tail
/// (which would make a buffer full of unterminated introducers quadratic). The
/// OSC terminator search is likewise bounded — it stops at the first bare ESC,
/// so it never scans past the introducer's own (short) run.
pub(in crate::mux) fn strip_replayable_rich_content(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let n = bytes.len();
    // Smallest index at or after which an `ESC \` (ST) terminator may still
    // exist. Once a terminator search runs off the end we set this to `n`, so
    // subsequent APC/DCS introducers short-circuit instead of re-scanning the
    // tail — that is what keeps the whole pass O(n).
    let mut st_search_from = 0usize;
    while i < n {
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
                    out.extend_from_slice(&strip.embedded_c0);
                    i = strip.end;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    out
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
fn is_replayable_osc_body(body: &[u8]) -> bool {
    // OSC 777 viewer launch: `777;emterm;<kind>;…`. Strip only the viewer
    // kinds and `agent-status` (SPEC FR4: the OSC report itself is never
    // replayed — the daemon resyncs current state out-of-band after a
    // snapshot); keep `fold` (fold marks) and any other kind (status-bar, …).
    if let Some(rest) = body.strip_prefix(b"777;emterm;") {
        let kind = rest.split(|&c| c == b';').next().unwrap_or(rest);
        if kind == AGENT_STATUS_OSC_KIND.as_bytes() {
            return true;
        }
        return REPLAYABLE_VIEWER_KINDS.iter().any(|k| kind == k.as_bytes());
    }
    // emterm Markdown OSC 9999: `9999;emterm-md;…`. Keep `emterm-mux;` (mux
    // control) and anything else.
    if body.starts_with(b"9999;emterm-md;") || body == b"9999;emterm-md" {
        return true;
    }
    false
}

/// Cap on the carry-over held by [`AgentStatusOscScanner`] for an in-flight
/// (not-yet-terminated) OSC body. A legitimate `agent-status` payload
/// (state + up to an 80-char percent-encoded name, SPEC NFR1) is always far
/// smaller than this; the cap exists purely to bound an adversarial or
/// wedged stream's memory footprint — an unterminated OSC introducer can
/// never pin unbounded bytes (task0012 AC-3/AC-4, review round-1 rework).
const AGENT_STATUS_SCANNER_CARRY_OVER_CAP: usize = 8 * 1024;

/// Decode state for [`AgentStatusOscScanner`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AgentStatusScanState {
    /// No partial OSC sequence in flight.
    Idle,
    /// Just consumed an ESC while `Idle`; waiting to see if the next byte is
    /// `]` (OSC introducer).
    SeenEsc,
    /// Inside `ESC ] <body>`, accumulating body bytes in `partial` (the
    /// introducer itself is not stored — `partial` holds body bytes only).
    InsideOsc,
    /// The most recent body byte was ESC, not yet pushed into `partial`,
    /// pending disambiguation: `\` completes ST, `]` reopens as a fresh OSC
    /// introducer (the just-seen ESC becomes ITS introducer), anything else
    /// aborts the in-flight OSC without emitting.
    InsideOscPendingSt,
}

/// Per-pane stateful decoder for the `agent-status` OSC 777 sequence (SPEC
/// FR1/FR3).
///
/// Unlike a stateless per-chunk scan, this scanner retains any incomplete
/// OSC sequence across [`Self::feed`] calls, so a report split across an
/// arbitrary PTY read boundary is still detected exactly once — the fix for
/// review round-1 stable_id `osc_split_lost` (a report could previously be
/// lost entirely if its terminator landed in a later `read()`).
///
/// One instance is owned per pane's reader thread ([`crate::mux::ipc::pty_spawn::pty_reader_loop`]),
/// mirroring how `ScrollbackWriteFilter` in that module owns its own
/// per-pane cross-chunk state.
///
/// Behavior:
/// - Both OSC terminators are recognized: BEL (`0x07`) and ST (`ESC \`).
/// - Only a complete body starting `777;emterm;agent-status;` produces an
///   event; any other OSC (a different `emterm` kind, or a foreign OSC
///   entirely) is recognized as *not our concern* and silently discarded
///   once terminated — no event, no error.
/// - A bare ESC encountered mid-body that is not the start of ST aborts the
///   in-flight OSC (nothing is emitted for it) and is immediately
///   re-examined as a fresh candidate introducer, so a following complete
///   `agent-status` OSC is still detected in the same `feed` call (mirrors
///   the position-independent semantics the previous stateless scan had
///   within one chunk).
/// - The body carry-over is capped at [`AGENT_STATUS_SCANNER_CARRY_OVER_CAP`]:
///   past that, the in-flight sequence is DROPPED (not emitted as a garbage
///   event) and the scanner resets to `Idle`, so it recovers cleanly on the
///   next well-formed sequence.
pub(in crate::mux) struct AgentStatusOscScanner {
    state: AgentStatusScanState,
    /// Body bytes accumulated for the in-flight OSC (introducer and
    /// terminator excluded).
    partial: Vec<u8>,
    /// True once a single carry-over-overflow warning has fired. Never
    /// re-armed, matching `PassthroughScanner`'s "warn once" behavior so a
    /// wedged/adversarial stream cannot spam the log.
    overflow_warned: bool,
}

impl AgentStatusOscScanner {
    pub(in crate::mux) fn new() -> Self {
        Self {
            state: AgentStatusScanState::Idle,
            partial: Vec::new(),
            overflow_warned: false,
        }
    }

    /// Feed one PTY reader chunk. Returns every complete `agent-status`
    /// report payload (`"emterm;agent-status;…"`,
    /// [`crate::agent_status::parse`]'s input contract) that completed
    /// during this call, in stream order. Any trailing incomplete OSC
    /// sequence is retained in `self` and resumed on the next `feed` call.
    pub(in crate::mux) fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in chunk {
            self.step(b, &mut out);
            if self.partial.len() > AGENT_STATUS_SCANNER_CARRY_OVER_CAP {
                if !self.overflow_warned {
                    log::warn!(
                        "agent-status OSC scanner: carry-over exceeded {} bytes; dropping in-flight sequence",
                        AGENT_STATUS_SCANNER_CARRY_OVER_CAP
                    );
                    self.overflow_warned = true;
                }
                self.reset();
            }
        }
        out
    }

    fn step(&mut self, b: u8, out: &mut Vec<String>) {
        match self.state {
            AgentStatusScanState::Idle => {
                if b == 0x1b {
                    self.state = AgentStatusScanState::SeenEsc;
                }
            }
            AgentStatusScanState::SeenEsc => match b {
                b']' => {
                    self.partial.clear();
                    self.state = AgentStatusScanState::InsideOsc;
                }
                0x1b => {
                    // Consecutive ESC: keep the latest one as the candidate
                    // introducer, stay in SeenEsc.
                }
                _ => {
                    // Not an OSC introducer; abandon the candidate.
                    self.state = AgentStatusScanState::Idle;
                }
            },
            AgentStatusScanState::InsideOsc => {
                if b == 0x07 {
                    // BEL terminator.
                    self.commit(out);
                } else if b == 0x1b {
                    // Could be the start of ST — hold it without pushing to
                    // `partial` until the next byte disambiguates.
                    self.state = AgentStatusScanState::InsideOscPendingSt;
                } else {
                    self.partial.push(b);
                }
            }
            AgentStatusScanState::InsideOscPendingSt => match b {
                b'\\' => {
                    // ST terminator (ESC \).
                    self.commit(out);
                }
                b']' => {
                    // The held ESC + this byte form a NEW OSC introducer —
                    // the old in-flight body is abandoned (never emitted),
                    // scanning restarts fresh from here.
                    self.partial.clear();
                    self.state = AgentStatusScanState::InsideOsc;
                }
                0x1b => {
                    // Still ambiguous; keep the latest ESC as the pending
                    // candidate.
                }
                _ => {
                    // The held ESC aborts the in-flight OSC (not the start
                    // of ST, not a fresh introducer); this byte itself is
                    // plain and starts nothing.
                    self.reset();
                }
            },
        }
    }

    /// Complete the in-flight OSC: emit an event only if the body is an
    /// `agent-status` report, then reset to `Idle` either way.
    fn commit(&mut self, out: &mut Vec<String>) {
        if let Some(rest) = self.partial.strip_prefix(b"777;emterm;agent-status;") {
            let mut payload = String::from("emterm;agent-status;");
            payload.push_str(&String::from_utf8_lossy(rest));
            out.push(payload);
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.partial.clear();
        self.state = AgentStatusScanState::Idle;
    }

    /// Current size of the in-flight carry-over buffer. Test-only observer.
    #[cfg(test)]
    fn carry_over_len(&self) -> usize {
        self.partial.len()
    }
}

impl Default for AgentStatusOscScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// A matched (strippable) CSI device query: where scanning resumes, and any
/// C0 control bytes embedded in the query body that must be re-emitted.
///
/// term_core's parser executes C0 controls encountered mid-CSI immediately
/// without aborting the sequence (`crates/term_core/src/parser/csi.rs`), so
/// dropping them along with the query would change replay behavior —
/// IMPLEMENTATION.md D2.
struct CsiStrip {
    /// Index just past the CSI final byte.
    end: usize,
    /// C0 control bytes (other than ESC) encountered inside the query body,
    /// in order.
    embedded_c0: Vec<u8>,
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
fn scan_csi_device_query(bytes: &[u8], from: usize) -> Option<CsiStrip> {
    let mut j = from;
    let mut private_prefix: Option<u8> = None;
    let mut first_param: u32 = 0;
    let mut collecting_first_param = true;
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
                // independently of the separate intermediates vec.
                if collecting_first_param && b.is_ascii_digit() {
                    // Saturating accumulation mirrors term_core's
                    // `ParamParser::add_digit` (saturating_mul/saturating_add
                    // — crates/term_core/src/parser_params.rs): an
                    // arbitrarily long digit run can never overflow or panic.
                    // A saturated value never equals a small target constant
                    // (5/6/14/16/18), so an oversized parameter is simply
                    // preserved, never stripped (the `n` arm additionally
                    // clamps to term_core's MAX_PARAM_VALUE before
                    // truncating to u8 — see `csi_is_device_query`).
                    first_param = first_param
                        .saturating_mul(10)
                        .saturating_add(u32::from(b - b'0'));
                } else {
                    // ';' / ':' / a digit past the first run — the leading
                    // decimal run is over either way.
                    collecting_first_param = false;
                }
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
                return if csi_is_device_query(private_prefix, &intermediates, first_param, b) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── agent-status strip (task0003 AC-3) ───────────────────────────────

    #[test]
    fn strip_removes_agent_status_set_report() {
        let input = b"before\x1b]777;emterm;agent-status;v=1;state=working;name=claude\x07after";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"beforeafter");
    }

    #[test]
    fn strip_removes_agent_status_clear_report() {
        let input = b"L\x1b]777;emterm;agent-status;clear\x1b\\R";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"LR");
    }

    #[test]
    fn strip_preserves_other_bytes_around_agent_status_report() {
        let mut input = Vec::new();
        input.extend_from_slice(b"$ emterm agent-status working\r\n");
        input.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=working\x07");
        input.extend_from_slice(b"$ next prompt");
        let out = strip_replayable_rich_content(&input);
        assert_eq!(
            out,
            b"$ emterm agent-status working\r\n$ next prompt".as_slice()
        );
    }

    // ── AgentStatusOscScanner (task0003 AC-3 / OSC detection; task0012
    //    per-pane statefulness rework) ─────────────────────────────────

    #[test]
    fn scan_extracts_single_set_report() {
        let input = b"pre\x1b]777;emterm;agent-status;v=1;state=blocked;name=claude\x07post";
        let out = AgentStatusOscScanner::new().feed(input);
        assert_eq!(
            out,
            vec!["emterm;agent-status;v=1;state=blocked;name=claude".to_string()]
        );
    }

    #[test]
    fn scan_extracts_clear_report_st_terminated() {
        let input = b"\x1b]777;emterm;agent-status;clear\x1b\\";
        let out = AgentStatusOscScanner::new().feed(input);
        assert_eq!(out, vec!["emterm;agent-status;clear".to_string()]);
    }

    #[test]
    fn scan_extracts_multiple_reports_in_order() {
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=working\x07");
        input.extend_from_slice(b"mid");
        input.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=done\x07");
        let out = AgentStatusOscScanner::new().feed(&input);
        assert_eq!(
            out,
            vec![
                "emterm;agent-status;v=1;state=working".to_string(),
                "emterm;agent-status;v=1;state=done".to_string(),
            ]
        );
    }

    #[test]
    fn scan_ignores_non_agent_status_osc() {
        let input = b"\x1b]777;emterm;markdown;begin\x07\x1b]0;title\x07";
        let out = AgentStatusOscScanner::new().feed(input);
        assert!(out.is_empty());
    }

    #[test]
    fn scan_ignores_unterminated_agent_status_osc() {
        let input = b"text\x1b]777;emterm;agent-status;v=1;state=working";
        let out = AgentStatusOscScanner::new().feed(input);
        assert!(out.is_empty());
    }

    #[test]
    fn scan_empty_input_returns_empty() {
        assert!(AgentStatusOscScanner::new().feed(b"").is_empty());
        assert!(
            AgentStatusOscScanner::new()
                .feed(b"plain text, no escapes")
                .is_empty()
        );
    }

    // ── task0012 AC-1: a report split across two chunks at every possible
    //    byte boundary results in exactly one decoded event ────────────

    #[test]
    fn scanner_feed_split_at_every_byte_boundary_yields_exactly_one_event() {
        let cases: [(&[u8], &str); 4] = [
            (
                b"\x1b]777;emterm;agent-status;v=1;state=working\x07",
                "emterm;agent-status;v=1;state=working",
            ),
            (
                b"\x1b]777;emterm;agent-status;v=1;state=blocked;name=claude\x1b\\",
                "emterm;agent-status;v=1;state=blocked;name=claude",
            ),
            (
                b"\x1b]777;emterm;agent-status;clear\x07",
                "emterm;agent-status;clear",
            ),
            (
                b"\x1b]777;emterm;agent-status;clear\x1b\\",
                "emterm;agent-status;clear",
            ),
        ];
        for (full, expected) in cases {
            for split in 1..full.len() {
                let mut scanner = AgentStatusOscScanner::new();
                let (head, tail) = full.split_at(split);
                let mut out = scanner.feed(head);
                out.extend(scanner.feed(tail));
                assert_eq!(
                    out,
                    vec![expected.to_string()],
                    "split at {split} of {full:?} must yield exactly one event"
                );
            }
        }
    }

    /// AC-1 variant: the split lands exactly on the two-byte ST terminator
    /// (`ESC \`) itself, so the first `feed` ends with a bare trailing ESC
    /// and the second `feed` begins with the lone `\`.
    #[test]
    fn scanner_feed_split_exactly_between_st_terminator_bytes() {
        let full = b"\x1b]777;emterm;agent-status;v=1;state=done\x1b\\";
        let split = full.len() - 1; // right before the trailing '\\'
        let (head, tail) = full.split_at(split);
        assert!(head.ends_with(b"\x1b"));
        let mut scanner = AgentStatusOscScanner::new();
        let mut out = scanner.feed(head);
        assert!(out.is_empty(), "no event before the ST terminator lands");
        out.extend(scanner.feed(tail));
        assert_eq!(out, vec!["emterm;agent-status;v=1;state=done".to_string()]);
    }

    // ── task0012 AC-3 / AC-4: bounded carry-over + clean recovery ───────

    #[test]
    fn scanner_bounds_carry_over_on_unterminated_prefix_plus_large_burst() {
        let mut scanner = AgentStatusOscScanner::new();
        let intro = b"\x1b]777;emterm;agent-status;v=1;state=working;name=";
        assert!(scanner.feed(intro).is_empty());
        // A large burst of plain (non-ESC) bytes with no terminator anywhere.
        let burst: Vec<u8> = std::iter::repeat_n(b'A', 200_000).collect();
        let out = scanner.feed(&burst);
        assert!(out.is_empty(), "still unterminated, so no event");
        assert!(
            scanner.carry_over_len() <= AGENT_STATUS_SCANNER_CARRY_OVER_CAP,
            "carry-over must stay bounded by the cap: got {}",
            scanner.carry_over_len()
        );
    }

    #[test]
    fn scanner_drops_overflowed_carry_over_without_garbage_event_and_recovers() {
        let mut scanner = AgentStatusOscScanner::new();
        let intro = b"\x1b]777;emterm;agent-status;v=1;state=working;name=";
        scanner.feed(intro);
        let burst: Vec<u8> =
            std::iter::repeat_n(b'A', AGENT_STATUS_SCANNER_CARRY_OVER_CAP + 1024).collect();
        let out = scanner.feed(&burst);
        assert!(
            out.is_empty(),
            "overflow must drop the in-flight sequence, not emit a garbage event"
        );
        assert_eq!(
            scanner.carry_over_len(),
            0,
            "carry-over must be reset after overflow"
        );

        // The scanner recovers cleanly: a subsequent well-formed report is
        // still detected in the same `feed` call.
        let out2 = scanner.feed(b"\x1b]777;emterm;agent-status;v=1;state=done\x07");
        assert_eq!(out2, vec!["emterm;agent-status;v=1;state=done".to_string()]);
    }

    // ── task0012: bare ESC mid-body aborts and is re-examined as a fresh
    //    introducer (matches the old stateless scan's per-position search) ─

    #[test]
    fn scanner_aborted_report_does_not_hide_a_following_complete_report() {
        // The first candidate never terminates (a CSI SGR sequence
        // interrupts it); the second, independent report must still be
        // detected in the same feed call.
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=work");
        input.extend_from_slice(b"\x1b[31m"); // unrelated CSI aborts the OSC body
        input.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=done\x07");
        let out = AgentStatusOscScanner::new().feed(&input);
        assert_eq!(out, vec!["emterm;agent-status;v=1;state=done".to_string()]);
    }

    // ── strip_replayable_rich_content unit tests ────────────────────────

    #[test]
    fn strip_removes_osc777_markdown_viewer() {
        let input = b"before\x1b]777;emterm;markdown;begin\x07after";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"beforeafter");
    }

    #[test]
    fn strip_removes_osc777_image_json_yaml_viewers() {
        for kind in [b"image".as_slice(), b"json".as_slice(), b"yaml".as_slice()] {
            let mut input = b"X\x1b]777;emterm;".to_vec();
            input.extend_from_slice(kind);
            input.extend_from_slice(b";chunk;DATA\x1b\\Y");
            let out = strip_replayable_rich_content(&input);
            assert_eq!(out, b"XY", "viewer kind {:?} must be stripped", kind);
        }
    }

    #[test]
    fn strip_keeps_osc777_fold_mark() {
        // fold marks are not viewer launches; they must be preserved.
        let input = b"L\x1b]777;emterm;fold;start;42\x07R";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_keeps_osc777_other_kinds() {
        // status-bar (or any non-viewer kind) must be preserved.
        let input = b"\x1b]777;emterm;status-bar;line\x07tail";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_removes_kitty_apc() {
        let input = b"pre\x1b_Gi=1,a=T;PAYLOAD\x1b\\post";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"prepost");
    }

    #[test]
    fn strip_removes_sixel_dcs() {
        let input = b"a\x1bP1;0;0q\"1;1;5;5#0;2;0;0;0\x1b\\b";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"ab");
    }

    #[test]
    fn strip_keeps_non_sixel_dcs() {
        // DCS without a 'q' final byte (e.g. DECRQSS reply) must be preserved.
        let input = b"\x1bP$tnotsixel\x1b\\";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_keeps_non_sixel_dcs_with_q_in_data() {
        // A non-SIXEL DCS whose *data* contains 'q' (0x71) must NOT be
        // stripped — only the DCS final byte being 'q' marks a SIXEL.
        // DECRQSS request: `DCS $ q <Pt> ST` would be SIXEL-like only if 'q'
        // were the final byte; here the final byte is '$' (intermediate is
        // skipped, '$' 0x24 is intermediate, so the first non-param,
        // non-intermediate byte is 't'). Use a clearer reply form.
        let input = b"\x1bP1$r0;1m\x1b\\"; // DECRQSS SGR reply, data has no 'q'
        assert_eq!(strip_replayable_rich_content(input), input);

        // And a DCS whose data literally contains 'q' but whose final byte is
        // not 'q': `DCS 0 $ r q-in-data ST`. Final byte after params(0) and
        // intermediates($) is 'r', so it is kept even though 'q' appears later.
        let input2 = b"\x1bP0$rabcq def\x1b\\";
        assert_eq!(strip_replayable_rich_content(input2), input2);
    }

    #[test]
    fn strip_removes_osc9999_emterm_md() {
        let input = b"head\x1b]9999;emterm-md;begin\x1b\\tail";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"headtail");
    }

    #[test]
    fn strip_removes_osc9999_emterm_md_bel_terminated() {
        let input = b"\x1b]9999;emterm-md;chunk;abc\x07";
        let out = strip_replayable_rich_content(input);
        assert!(out.is_empty());
    }

    #[test]
    fn strip_keeps_osc9999_emterm_mux_control() {
        // mux control (emterm-mux) is not a viewer; preserve it.
        let input = b"\x1b]9999;emterm-mux;state;1\x07X";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_preserves_plain_text_and_sgr() {
        let input = b"hello \x1b[31mred\x1b[0m world\r\n\x1b[?1049h\x1b[H\x1b[2Jmore";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_keeps_osc0_title() {
        let input = b"\x1b]0;my window title\x07prompt$ ";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_keeps_unterminated_partial_sequence() {
        // An OSC 777 viewer launch whose terminator never arrived must NOT
        // be dropped (we only strip completed sequences). This guarantees
        // plain text is never accidentally truncated.
        let input = b"text\x1b]777;emterm;markdown;begin-no-terminator";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_handles_both_terminators_in_one_run() {
        let mut input = Vec::new();
        input.extend_from_slice(b"A");
        input.extend_from_slice(b"\x1b]777;emterm;markdown;x\x07"); // BEL
        input.extend_from_slice(b"B");
        input.extend_from_slice(b"\x1b]777;emterm;json;y\x1b\\"); // ST
        input.extend_from_slice(b"C");
        let out = strip_replayable_rich_content(&input);
        assert_eq!(out, b"ABC");
    }

    #[test]
    fn strip_removes_mixed_rich_content_keeps_text() {
        let mut input = Vec::new();
        input.extend_from_slice(b"$ emterm markdown README.md\r\n");
        input.extend_from_slice(b"\x1b]777;emterm;markdown;begin\x07");
        input.extend_from_slice(b"\x1b_Gi=1;IMG\x1b\\");
        input.extend_from_slice(b"\x1b]9999;emterm-md;chunk;c\x07");
        input.extend_from_slice(b"$ next prompt");
        let out = strip_replayable_rich_content(&input);
        assert_eq!(out, b"$ emterm markdown README.md\r\n$ next prompt");
    }

    /// Performance / correctness: a scrollback full of unterminated APC / DCS
    /// introducers must complete in a single O(n) pass (no quadratic re-scan)
    /// and preserve every byte (the introducers are partial sequences).
    #[test]
    fn strip_unterminated_introducers_complete_in_single_pass() {
        // Thousands of `ESC _ G` / `ESC P` introducers with NO ST terminator
        // anywhere. The old implementation re-scanned the tail for every one,
        // making this O(n²); the cached `st_search_from` makes it O(n).
        let mut input = Vec::new();
        for _ in 0..20_000 {
            input.extend_from_slice(b"\x1b_G"); // APC introducer, no terminator
            input.extend_from_slice(b"\x1bP1;0;0q"); // DCS/SIXEL introducer, no terminator
            input.extend_from_slice(b"plain");
        }
        let out = strip_replayable_rich_content(&input);
        // Nothing is terminated, so nothing is stripped — output equals input.
        assert_eq!(out, input);
    }

    /// Perf bench: measure `strip_replayable_rich_content` on a 2 MiB
    /// scrollback dominated by plain text (the `seq 1 N` shape — no ESC
    /// sequences at all). This is the snapshot-rebuild hot path: a tab switch
    /// runs this on the full 2 MiB ring once per attach.
    ///
    /// Gated `#[ignore]` so it does not run by default. Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path src-tauri/Cargo.toml --lib --features gui \
    ///   strip_replayable_rich_content_bench_2mib_plain \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn strip_replayable_rich_content_bench_2mib_plain() {
        use std::time::Instant;
        // Build ~2 MiB of `seq 1 N`-shaped output: 7-digit decimal + "\r\n".
        let mut input = Vec::with_capacity(2 * 1024 * 1024);
        let mut n: u64 = 1;
        while input.len() < 2 * 1024 * 1024 {
            use std::io::Write;
            let _ = write!(&mut input, "{n}\r\n");
            n += 1;
        }
        input.truncate(2 * 1024 * 1024);
        // Warm-up so allocator + I-cache are hot.
        for _ in 0..2 {
            let _ = strip_replayable_rich_content(&input);
        }
        let iters = 5;
        let start = Instant::now();
        for _ in 0..iters {
            let out = strip_replayable_rich_content(&input);
            std::hint::black_box(out);
        }
        let elapsed = start.elapsed();
        let per = elapsed / iters as u32;
        eprintln!(
            "[bench] strip_replayable_rich_content 2MiB plain: {iters} iters / {:?} → {:?}/call ({:.1} MiB/s)",
            elapsed,
            per,
            (2.0 * iters as f64) / elapsed.as_secs_f64(),
        );
        // SPEC.md "Performance Goals" (FR5): the stripper must stay well
        // under the snapshot-replay budget on a 2 MiB plain payload.
        let threshold = std::time::Duration::from_millis(30);
        assert!(
            per < threshold,
            "strip_replayable_rich_content per-call {:?} ≥ threshold {:?} (FR5)",
            per,
            threshold,
        );
    }

    /// drift guard (a): the OSC 777 stripper must key off exactly the shared
    /// [`REPLAYABLE_VIEWER_KINDS`] SSOT, and every one of those kinds must in
    /// fact be stripped (and a non-listed kind must be kept). If a kind is
    /// added to the SSOT, this test confirms the stripper picks it up.
    #[test]
    fn strip_matches_replayable_viewer_kinds_ssot() {
        for kind in REPLAYABLE_VIEWER_KINDS {
            let mut input = b"\x1b]777;emterm;".to_vec();
            input.extend_from_slice(kind.as_bytes());
            input.extend_from_slice(b";begin\x07");
            assert!(
                strip_replayable_rich_content(&input).is_empty(),
                "SSOT viewer kind {kind:?} must be stripped"
            );
        }
        // A kind NOT in the SSOT (e.g. fold) is kept.
        assert!(!REPLAYABLE_VIEWER_KINDS.contains(&"fold"));
        let kept = b"\x1b]777;emterm;fold;x\x07".to_vec();
        assert_eq!(strip_replayable_rich_content(&kept), kept);
    }

    // ── CSI device-query strip tests (AC-1 … AC-10) ─────────────────────

    /// AC-1: DA1 forms (`ESC[c`, `ESC[0c`, `ESC[?…c`) and DA2 forms
    /// (`ESC[>c`, `ESC[>0c`) are removed; surrounding bytes preserved.
    #[test]
    fn strip_removes_da1_and_da2_queries() {
        for input in [
            b"a\x1b[cb".as_slice(),
            b"a\x1b[0cb".as_slice(),
            b"a\x1b[?1;2cb".as_slice(),
            b"a\x1b[>cb".as_slice(),
            b"a\x1b[>0cb".as_slice(),
        ] {
            let out = strip_replayable_rich_content(input);
            assert_eq!(
                out, b"ab",
                "input {input:?} must be stripped to just surrounding text"
            );
        }
    }

    /// AC-2: `ESC[5n` and `ESC[6n` are removed; `ESC[0n` and `ESC[?6n` are
    /// preserved.
    #[test]
    fn strip_removes_dsr_and_cpr_queries_keeps_others() {
        assert_eq!(strip_replayable_rich_content(b"a\x1b[5nb"), b"ab");
        assert_eq!(strip_replayable_rich_content(b"a\x1b[6nb"), b"ab");
        let unanswered = b"a\x1b[0nb";
        assert_eq!(strip_replayable_rich_content(unanswered), unanswered);
        let private = b"a\x1b[?6nb";
        assert_eq!(strip_replayable_rich_content(private), private);
    }

    /// AC-3: `ESC[14t`, `ESC[16t`, `ESC[18t` are removed; `ESC[22t`,
    /// `ESC[23t`, `ESC[8;24;80t` are preserved.
    #[test]
    fn strip_removes_xtwinops_size_reports_keeps_others() {
        for ps in [14, 16, 18] {
            let input = format!("a\x1b[{ps}tb").into_bytes();
            let out = strip_replayable_rich_content(&input);
            assert_eq!(out, b"ab", "Ps={ps} must be stripped");
        }
        for suffix in ["22t", "23t", "8;24;80t"] {
            let input = format!("a\x1b[{suffix}b").into_bytes();
            assert_eq!(
                strip_replayable_rich_content(&input),
                input,
                "ESC[{suffix} must be preserved"
            );
        }
    }

    /// AC-4: `ESC[?Ps$p` (known and unknown modes) is removed; `ESC[!p` and
    /// `ESC["p` are preserved.
    #[test]
    fn strip_removes_decrpm_keeps_non_decrpm_p_final() {
        // Known mode (2026 = synchronized output) and an unknown mode.
        assert_eq!(strip_replayable_rich_content(b"a\x1b[?2026$pb"), b"ab");
        assert_eq!(strip_replayable_rich_content(b"a\x1b[?9999$pb"), b"ab");

        let bang = b"a\x1b[!pb";
        assert_eq!(strip_replayable_rich_content(bang), bang);
        let quote = b"a\x1b[\"pb";
        assert_eq!(strip_replayable_rich_content(quote), quote);
    }

    /// AC-5: `ESC[=c` (DA3 — term_core does not answer it) is preserved.
    #[test]
    fn strip_keeps_da3_tertiary_device_attributes() {
        let input = b"a\x1b[=cb";
        assert_eq!(strip_replayable_rich_content(input), input);
    }

    /// AC-6: an unterminated CSI at end of buffer is preserved.
    #[test]
    fn strip_keeps_unterminated_csi_device_query() {
        let input = b"text\x1b[5"; // DSR query missing its final byte
        assert_eq!(strip_replayable_rich_content(input), input);
    }

    /// AC-7: a stripped query containing an embedded C0 byte re-emits that
    /// byte (BEL survives; the query bytes do not).
    #[test]
    fn strip_removes_csi_query_reemits_embedded_c0() {
        let input = b"before\x1b[5\x07nafter"; // BEL embedded mid-DSR-query
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"before\x07after");
    }

    /// AC-8: a bare ESC inside a CSI body aborts the candidate — the prefix
    /// is preserved and a following complete query is still stripped.
    #[test]
    fn strip_bare_esc_in_csi_body_aborts_then_strips_following_query() {
        // "\x1b[5" has no final byte before a fresh ESC starts a new CSI;
        // the aborted prefix is kept and the following ESC[6n is stripped.
        let input = b"\x1b[5\x1b[6n";
        let out = strip_replayable_rich_content(input);
        assert_eq!(out, b"\x1b[5");
    }

    /// AC-9: a mixed payload of plain text, SGR, viewer OSC, and device
    /// queries removes only the viewer OSC + queries.
    #[test]
    fn strip_removes_mixed_osc_and_csi_queries_keeps_text_and_sgr() {
        let mut input = Vec::new();
        input.extend_from_slice(b"$ prompt\x1b[31mred\x1b[0m\r\n");
        input.extend_from_slice(b"\x1b]777;emterm;markdown;begin\x07");
        input.extend_from_slice(b"\x1b[c"); // DA1 query
        input.extend_from_slice(b"\x1b[6n"); // CPR query
        input.extend_from_slice(b"more text");
        let out = strip_replayable_rich_content(&input);
        assert_eq!(out, b"$ prompt\x1b[31mred\x1b[0m\r\nmore text");
    }

    /// AC-10 (funnel regression, SPEC TS-12): a full `build_snapshot_bytes`
    /// product built from a DA1-bearing scrollback contains no removable
    /// device query.
    #[test]
    fn build_snapshot_bytes_funnel_strips_da1_device_query() {
        use crate::mux::snapshot_bytes::build_snapshot_bytes;
        let scrollback = b"prompt$ \x1b[cdone"; // DA1 query in scrollback
        let out = build_snapshot_bytes(scrollback, b"", false);
        assert!(
            !out.windows(3).any(|w| w == b"\x1b[c"),
            "snapshot must not contain a removable DA1 device query: {out:?}"
        );
        assert!(
            out.windows(6).any(|w| w == b"prompt"),
            "surrounding plain text must survive: {out:?}"
        );
        assert!(
            out.windows(4).any(|w| w == b"done"),
            "surrounding plain text must survive: {out:?}"
        );
    }

    // ── review round 1 rework regression tests (task0002 AC-1 … AC-5) ──

    /// task0002 AC-1: a first CSI parameter with more than 10 digits must be
    /// preserved (a saturated accumulator never equals a small target
    /// constant) and must not panic under overflow-checked builds — mirrors
    /// term_core's saturating `ParamParser::add_digit`
    /// (`crates/term_core/src/parser_params.rs`).
    #[test]
    fn strip_keeps_oversized_first_param_no_panic() {
        let input = b"a\x1b[99999999999nb"; // 11-digit run, far beyond u32::MAX
        assert_eq!(strip_replayable_rich_content(input), input);
    }

    /// task0002 AC-2: DA1/DA2 with a private marker (`?`/`>`) as the FIRST
    /// intermediate must be stripped regardless of trailing intermediate
    /// bytes — term_core dispatches on `intermediates.first()` only
    /// (`crates/term_core/src/csi_dispatch.rs`).
    #[test]
    fn strip_removes_da_with_private_marker_and_trailing_intermediate() {
        for input in [
            b"a\x1b[?1$cb".as_slice(),
            b"a\x1b[?1!cb".as_slice(),
            b"a\x1b[> cb".as_slice(),
        ] {
            assert_eq!(
                strip_replayable_rich_content(input),
                b"ab",
                "input {input:?} must be stripped"
            );
        }
    }

    /// task0002 AC-3: DECRPM must be stripped when the first intermediate is
    /// `$`, regardless of further intermediate bytes beyond it (term_core
    /// truncates the collected intermediates to `MAX_CSI_INTERMEDIATES = 2`
    /// and only checks slot 1).
    #[test]
    fn strip_removes_decrpm_with_trailing_intermediate_bytes() {
        for input in [b"a\x1b[?2026$$pb".as_slice(), b"a\x1b[?2026$ pb".as_slice()] {
            assert_eq!(
                strip_replayable_rich_content(input),
                b"ab",
                "input {input:?} must be stripped"
            );
        }
    }

    /// task0002 AC-4: DA3 (`ESC[=c`), non-DECRPM `p` finals (`ESC[!p`,
    /// `ESC["p`), and a `c` final whose FIRST intermediate is not a private
    /// marker (`ESC[!c`) are never answered by term_core and must be
    /// preserved.
    #[test]
    fn strip_keeps_da3_non_decrpm_p_and_non_private_c() {
        for input in [
            b"a\x1b[=cb".as_slice(),
            b"a\x1b[!pb".as_slice(),
            b"a\x1b[\"pb".as_slice(),
            b"a\x1b[!cb".as_slice(),
        ] {
            assert_eq!(
                strip_replayable_rich_content(input),
                input,
                "input {input:?} must be preserved"
            );
        }
    }

    // ── review round 2 rework regression tests (task0003 AC-1 … AC-3) ──

    /// task0003 AC-1 (round 2 finding 864ff69541b6bcf8): term_core
    /// dispatches DSR as
    /// `handle_device_status_report(get_first_or_zero(params) as u8)`
    /// (csi_dispatch.rs) — the clamped first parameter is truncated to u8
    /// before the 5/6 match. `ESC[261n` (261 mod 256 = 5) and `ESC[262n`
    /// (262 mod 256 = 6) must be stripped; `ESC[260n` (mod 256 = 4) and
    /// `ESC[9999n` (mod 256 = 15) alias to neither 5 nor 6 and must be
    /// preserved.
    #[test]
    fn strip_removes_dsr_via_u8_truncated_param_keeps_non_aliasing_values() {
        assert_eq!(strip_replayable_rich_content(b"a\x1b[261nb"), b"ab");
        assert_eq!(strip_replayable_rich_content(b"a\x1b[262nb"), b"ab");
        let kept_260 = b"a\x1b[260nb";
        assert_eq!(strip_replayable_rich_content(kept_260), kept_260);
        let kept_9999 = b"a\x1b[9999nb";
        assert_eq!(strip_replayable_rich_content(kept_9999), kept_9999);
    }

    /// task0003 AC-2 (round 2 finding 445cfc21db4c4741): term_core's
    /// `csi_param` state keeps accepting parameter digits and `;`/`:` after
    /// an intermediate byte — they still feed the same `ParamParser`
    /// (`parser/csi.rs`), so `ESC[?$1c` still dispatches DA1 (intermediates
    /// `[?, $]`) and must be stripped. A DECRPM form with a digit after `$`
    /// dispatches the same way — csi_dispatch.rs's DECRPM arm only checks
    /// `intermediates.get(1) == Some(&'$')`, independent of trailing
    /// digits — so `ESC[?2026$1p` must also be stripped.
    #[test]
    fn strip_removes_da1_and_decrpm_with_digit_after_intermediate() {
        assert_eq!(strip_replayable_rich_content(b"a\x1b[?$1cb"), b"ab");
        assert_eq!(strip_replayable_rich_content(b"a\x1b[?2026$1pb"), b"ab");
    }

    /// task0003 AC-3 (round 2 finding ed8f3f3e4759734b): a private marker
    /// byte is valid only as term_core's `csi_entry`-state leading byte.
    /// Once any digit, separator, or intermediate has been seen, a private
    /// marker hits `csi_param`'s invalid-byte arm and cancels the whole CSI
    /// — no dispatch, no response (`parser/csi.rs`). `ESC[5?n` and
    /// `ESC[0?c` must therefore be preserved byte-for-byte, not stripped.
    #[test]
    fn strip_keeps_non_leading_private_marker_cancelled_csi() {
        let dsr = b"a\x1b[5?nb";
        assert_eq!(strip_replayable_rich_content(dsr), dsr);
        let da1 = b"a\x1b[0?cb";
        assert_eq!(strip_replayable_rich_content(da1), da1);
    }
}
