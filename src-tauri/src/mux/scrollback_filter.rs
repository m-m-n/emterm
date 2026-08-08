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
/// An offset falling strictly inside a sequence this pass REMOVES maps to
/// the output position immediately after whatever content preceded it (the
/// removed span contributes nothing, so nothing exists there any more — the
/// closest faithful stand-in). An offset at or past `bytes.len()` maps to
/// `out.len()` (the end of the stripped output).
pub(in crate::mux) fn strip_rich_content_and_remap(
    bytes: &[u8],
    watch_offsets: &[usize],
) -> (Vec<u8>, Vec<usize>) {
    let mut out = Vec::with_capacity(bytes.len());
    let mut remapped = vec![0usize; watch_offsets.len()];
    let mut next_watch = 0usize;
    let mut i = 0;
    let n = bytes.len();
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
            remapped[next_watch] = out.len();
            next_watch += 1;
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
    // Any remaining watch offsets (including one exactly at `bytes.len()`,
    // which the loop above never revisits since it exits at `i == n`) map
    // to the final output length.
    while next_watch < watch_offsets.len() {
        remapped[next_watch] = out.len();
        next_watch += 1;
    }
    (out, remapped)
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
/// Identical for the write path and the snapshot path (task0004 round-4
/// rework D1' — see [`strip_pty_output_for_scrollback_write`]'s doc
/// comment): there is no more `resize` kind to conditionally strip.
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
mod tests;
