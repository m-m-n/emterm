//! Replacement payload builder for a suppressed chunk (mux-snapshot-output-boundary
//! task0001, IMPLEMENTATION.md Shared Components "Replacement payload for a
//! suppressed chunk").
//!
//! When the PTY reader suppresses a chunk for a destination that a delivered
//! snapshot already covers (FR3/FR11), two guarantees still have to hold for
//! that destination:
//!
//! - **FR9**: any complete terminal query (device query or OSC color query)
//!   inside the suppressed chunk that does NOT survive into the snapshot
//!   bytes must still reach the client once, so a program blocked on that
//!   query's response is not left waiting forever.
//! - **FR10**: an incomplete control sequence or UTF-8 character cut off at
//!   the suppressed chunk's boundary must not have its continuation (in a
//!   later, delivered chunk) misread as literal text or a stray replacement
//!   character.
//!
//! [`build_suppressed_replacement`] answers both by scanning the suppressed
//! chunk ONCE, left to right, and returning `Q ++ T`: `Q` is every
//! qualifying complete query found outside the tail region, in original
//! order; `T` is the tail itself (either the write filter's still-pending
//! run, or the chunk's own incomplete trailing sequence). The result is
//! delivered as an ordinary `PtyOutput` chunk to the suppressed chunk's
//! destination, after the snapshot and before the reader's next chunk — see
//! `pty_reader_loop`'s call site.
//!
//! TM-1 (never fabricate a query): a query only counts where the client's
//! parser would actually start a control sequence. Bytes inside the payload
//! of an OSC, DCS, APC, SOS or PM string — or inside the tail region — are
//! never inspected for a nested query; those strings are skipped over as
//! opaque runs. TM-2 (bounded, never empty): the scan is a single forward
//! pass with no rescanning from an already-visited byte, this function does
//! no work at all for a chunk that was not suppressed (callers only invoke
//! it for suppressed chunks), and an empty result is never turned into an
//! empty `PtyOutput` chunk by the caller (that would read as PTY exit).

use std::ops::Range;

use crate::mux::scrollback_filter::scan_csi_device_query;

/// Build the FR9/FR10 replacement payload for a suppressed chunk.
///
/// - `chunk`: the suppressed chunk's raw bytes (before any stripping).
/// - `ring_written_ranges`: the byte ranges of `chunk` that were fed toward
///   the scrollback ring this read (the reader's `live_spans` /
///   `main_spans` — main-buffer content; alternate-screen spans are NOT
///   included). Used only to decide which OSC color queries are absent
///   from the snapshot (D7): one inside these ranges survived into the
///   ring byte-for-byte (the write filter does not strip color queries),
///   so re-delivering it would duplicate what the snapshot already carries.
/// - `pending_after`: [`super::write_filter::ScrollbackWriteFilter::pending`]'s
///   contents taken right after this chunk was fed to the filter. Never
///   empty unless nothing is currently held back.
///
/// Returns `Q ++ T`, which may be empty (see the module doc's TM-2 note —
/// an empty result must never itself become an empty `PtyOutput` chunk).
pub(in crate::mux) fn build_suppressed_replacement(
    chunk: &[u8],
    ring_written_ranges: &[Range<usize>],
    pending_after: &[u8],
) -> Vec<u8> {
    if chunk.is_empty() {
        return Vec::new();
    }

    // D8: the write filter's pending run — when non-empty — IS the tail,
    // verbatim, even when it carries bytes from an earlier chunk (it only
    // ever holds a not-yet-terminated rich-content introducer: OSC/DCS/APC,
    // never a CSI or bare UTF-8 tail — see `find_safe_boundary`'s doc in
    // `write_filter.rs`). When it is non-empty we still need to know how
    // much of THIS chunk's own `ring_written_ranges` contribution was
    // swallowed into it, so the query scan below never re-inspects bytes
    // that are only a not-yet-classified candidate, not a completed query
    // (TM-1).
    let (scan_limit, tail_from_pending): (usize, Option<Vec<u8>>) = if !pending_after.is_empty() {
        let to_write_len: usize = ring_written_ranges.iter().map(|r| r.end - r.start).sum();
        let contribution = pending_after.len().min(to_write_len);
        let limit = if contribution == 0 {
            chunk.len()
        } else {
            tail_exclusion_start(ring_written_ranges, contribution)
        };
        (limit, Some(pending_after.to_vec()))
    } else {
        (chunk.len(), None)
    };

    let (mut out, discovered_tail_start) = scan_for_queries(chunk, scan_limit, ring_written_ranges);

    let tail_bytes: Vec<u8> = match tail_from_pending {
        Some(pending) => pending,
        None => match discovered_tail_start {
            Some(start) => chunk[start..].to_vec(),
            None => Vec::new(),
        },
    };
    out.extend_from_slice(&tail_bytes);
    out
}

/// Given `ranges` (ascending, non-overlapping byte ranges of some buffer)
/// whose concatenation is `to_write`, return the position in the ORIGINAL
/// buffer's coordinates where the trailing `contribution`-byte suffix of
/// that concatenation begins. `contribution` must be `<= ` the ranges'
/// total length (the caller clamps it with `.min(to_write_len)`).
fn tail_exclusion_start(ranges: &[Range<usize>], contribution: usize) -> usize {
    let mut remaining = contribution;
    for r in ranges.iter().rev() {
        let len = r.end - r.start;
        if len >= remaining {
            return r.end - remaining;
        }
        remaining -= len;
    }
    0
}

/// Single forward-pass scan of `chunk[..limit]` for FR9-qualifying queries.
///
/// Returns `(Q bytes in original order, tail start)`. `tail start` is only
/// ever `Some` when `limit == chunk.len()` (the caller has no pending run,
/// so this scan is free to discover the chunk's OWN incomplete trailing
/// sequence per D8) — it is the position where an incomplete CSI,
/// ESC-intermediate sequence, OSC, DCS, APC/SOS/PM string, or partial UTF-8
/// character begins and runs to `chunk.len()` without ever completing.
///
/// TM-1: an OSC/DCS/APC/SOS/PM string's body is only ever inspected for the
/// OSC color-query grammar on ITS OWN top-level introducer; every other
/// string body is skipped wholesale by jumping straight to its terminator,
/// so a query-shaped byte run nested inside one is never extracted.
fn scan_for_queries(
    chunk: &[u8],
    limit: usize,
    ring_written_ranges: &[Range<usize>],
) -> (Vec<u8>, Option<usize>) {
    let detect_tail = limit == chunk.len();
    let mut q = Vec::new();
    let mut pos = 0usize;

    while pos < limit {
        let b = chunk[pos];
        if b != 0x1b {
            pos += 1;
            continue;
        }
        match chunk.get(pos + 1) {
            None => {
                // Bare ESC as the very last byte we're allowed to look at.
                if detect_tail {
                    return (q, Some(pos));
                }
                pos += 1;
            }
            Some(b'[') => match scan_csi_device_query(chunk, pos + 2) {
                Some(strip) if strip.end <= limit => {
                    // D7: re-deliver the query with any embedded C0 control
                    // byte removed — the snapshot's own strip already
                    // re-emits those bytes in place, so including them here
                    // too would execute them twice. C0 values (0x00-0x1A,
                    // 0x1C-0x1F) never overlap a valid CSI param,
                    // intermediate, private-marker or final byte, so a
                    // value-based filter over the matched span cannot drop
                    // anything else.
                    q.extend(
                        chunk[pos..strip.end]
                            .iter()
                            .copied()
                            .filter(|&c| !matches!(c, 0x00..=0x1a | 0x1c..=0x1f)),
                    );
                    pos = strip.end;
                }
                _ => {
                    if detect_tail && is_unterminated_csi_tail(chunk, pos) {
                        return (q, Some(pos));
                    }
                    pos += 1;
                }
            },
            Some(b']') => match find_osc_terminator(chunk, pos + 2) {
                OscScanResult::Complete(body_end, term_end) if term_end <= limit => {
                    let body = &chunk[pos + 2..body_end];
                    if is_color_query(body) && !overlaps_ranges(pos, term_end, ring_written_ranges)
                    {
                        q.extend_from_slice(&chunk[pos..term_end]);
                    }
                    pos = term_end;
                }
                OscScanResult::Complete(..) => {
                    // Matched but extends past our limit (a rare
                    // boundary-adjacent case) — conservative, skip.
                    pos += 1;
                }
                OscScanResult::Aborted(abort_pos) => {
                    // A bare ESC (not `ESC \`) cancels the in-flight OSC —
                    // this mirrors the client parser (and this crate's own
                    // `write_filter::find_osc_end`): the OSC never
                    // completed, so nothing inside it was ever "inside a
                    // string payload" to begin with. Not a tail either (the
                    // buffer did not run out) — resume scanning AT the
                    // aborting ESC as a fresh top-level position.
                    pos = abort_pos;
                }
                OscScanResult::Unterminated => {
                    if detect_tail {
                        return (q, Some(pos));
                    }
                    // Unterminated and not at the tail: nothing after this
                    // point in `chunk[..limit]` is safely scannable as a
                    // fresh top-level sequence (the OSC body would swallow
                    // it), so stop rather than risk scanning INTO the
                    // string's own payload (TM-1).
                    break;
                }
            },
            Some(b'P') | Some(b'_') | Some(b'X') | Some(b'^') => {
                // DCS / APC / SOS / PM: opaque string, skipped wholesale —
                // never inspected for a nested query (TM-1).
                match find_st(chunk, pos + 2) {
                    Some(end) if end <= limit => {
                        pos = end;
                    }
                    _ => {
                        if detect_tail {
                            return (q, Some(pos));
                        }
                        break;
                    }
                }
            }
            Some(&next) if (0x20..=0x2f).contains(&next) => {
                // A non-string ESC-intermediate sequence (e.g. character-set
                // designation `ESC ( B`). If, in tail-detection mode, every
                // byte from here to `limit` stays in the intermediate range
                // with no final byte (0x30-0x7E) ever appearing, this is an
                // incomplete escape sequence running off the chunk's end
                // (D8's "ESC with intermediates").
                if detect_tail {
                    let mut j = pos + 1;
                    let mut complete = false;
                    while j < limit {
                        match chunk[j] {
                            0x20..=0x2f => j += 1,
                            0x30..=0x7e => {
                                complete = true;
                                break;
                            }
                            _ => break,
                        }
                    }
                    if !complete && j >= limit {
                        return (q, Some(pos));
                    }
                }
                pos += 1;
            }
            Some(_) => {
                pos += 1;
            }
        }
    }

    if detect_tail {
        if let Some(start) = utf8_tail_start_at_end(&chunk[..limit]) {
            return (q, Some(start));
        }
    }
    (q, None)
}

/// Whether the CSI candidate starting at `esc_pos` (`chunk[esc_pos] ==
/// ESC`, `chunk[esc_pos + 1] == '['`) is incomplete because `chunk` ends
/// before a final byte (0x40-0x7E) or an aborting bare ESC is seen — i.e.
/// genuinely cut off at the chunk boundary (D8), as opposed to
/// [`scan_csi_device_query`] returning `None` for a CSI that completed
/// within the chunk but simply did not match the device-query predicate.
fn is_unterminated_csi_tail(chunk: &[u8], esc_pos: usize) -> bool {
    let mut j = esc_pos + 2;
    while j < chunk.len() {
        let b = chunk[j];
        if b == 0x1b {
            return false;
        }
        if (0x40..=0x7e).contains(&b) {
            return false;
        }
        j += 1;
    }
    true
}

/// Outcome of scanning for an OSC's terminator (see [`find_osc_terminator`]).
enum OscScanResult {
    /// `(index of the terminator's first byte, index just past the
    /// terminator)`.
    Complete(usize, usize),
    /// A bare ESC (not followed by `\\`) was seen at this index — the OSC
    /// is cancelled there, NOT unterminated (the buffer did not run out).
    Aborted(usize),
    /// The buffer ran out before BEL or ST appeared.
    Unterminated,
}

/// Find an OSC's terminator (BEL or ST) starting at `from`. A bare ESC not
/// followed by `\\` aborts the scan (mirrors `write_filter::find_osc_end` /
/// `scrollback_filter`'s own terminator scan — reimplemented locally rather
/// than widening either module's visibility further than this task's scope
/// calls for) — this is a real terminal-parser convention (an escape
/// cancels the in-flight string and begins a fresh attempt), not a bug: an
/// aborted OSC's "body" was never actually a completed string, so nothing
/// in it was ever protected from top-level recognition (TM-1's guarantee is
/// about COMPLETED string payloads).
fn find_osc_terminator(bytes: &[u8], from: usize) -> OscScanResult {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == 0x07 {
            return OscScanResult::Complete(j, j + 1);
        }
        if bytes[j] == 0x1b {
            if j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                return OscScanResult::Complete(j, j + 2);
            }
            return OscScanResult::Aborted(j);
        }
        j += 1;
    }
    OscScanResult::Unterminated
}

/// Find the index just past an ST (`ESC \`) terminator starting at or after
/// `from`. Used for DCS / APC / SOS / PM strings, which (unlike OSC) do not
/// accept a bare BEL terminator.
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

/// Whether an OSC body matches the color-query grammar (D7): a leading
/// field of `4`, `10`, `11` or `12`, and a trailing field of exactly `?`.
/// `4` additionally carries a color-index field in real usage
/// (`4;<index>;?`), so this only requires at least two fields total rather
/// than pinning the exact count, while still rejecting anything whose last
/// field isn't a bare `?`.
fn is_color_query(body: &[u8]) -> bool {
    let mut fields = body.split(|&b| b == b';');
    let Some(code) = fields.next() else {
        return false;
    };
    if !matches!(code, b"4" | b"10" | b"11" | b"12") {
        return false;
    }
    let mut last = None;
    for f in fields {
        last = Some(f);
    }
    last == Some(b"?".as_slice())
}

/// Whether `[start, end)` overlaps any range in `ranges`.
fn overlaps_ranges(start: usize, end: usize, ranges: &[Range<usize>]) -> bool {
    ranges.iter().any(|r| start < r.end && end > r.start)
}

/// Whether `bytes` ends with an incomplete multi-byte UTF-8 sequence — a
/// lead byte within the last 3 bytes that expects more continuation bytes
/// than remain, with every byte after it a valid continuation byte
/// (0x80-0xBF). Returns the lead byte's position if so. A bounded O(1)
/// check (at most 4 bytes examined), run once per scan rather than at every
/// byte position.
fn utf8_tail_start_at_end(bytes: &[u8]) -> Option<usize> {
    let len = bytes.len();
    let probe = 4.min(len);
    for back in 1..=probe {
        let start = len - back;
        let b = bytes[start];
        let expected_len = match b {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => continue,
        };
        if expected_len > back
            && bytes[start + 1..]
                .iter()
                .all(|&c| (0x80..=0xbf).contains(&c))
        {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chunk_yields_empty_result() {
        assert!(build_suppressed_replacement(&[], &[], &[]).is_empty());
    }

    #[test]
    fn plain_output_with_no_query_and_no_tail_yields_empty_result() {
        let chunk = b"hello, world\r\n";
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert!(
            result.is_empty(),
            "no query, no tail => nothing to re-deliver"
        );
    }

    // ---- FR9: CSI device queries ----

    #[test]
    fn csi_cursor_position_query_is_redelivered() {
        let chunk = b"\x1b[6n";
        let result = build_suppressed_replacement(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn csi_primary_device_attributes_query_is_redelivered() {
        let chunk = b"\x1b[c";
        let result = build_suppressed_replacement(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn csi_device_query_surrounded_by_plain_text_is_extracted_alone() {
        let chunk = b"before\x1b[6nafter";
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b[6n");
    }

    #[test]
    fn non_query_csi_is_not_redelivered() {
        // A complete, non-query CSI (cursor-up) must not be re-delivered —
        // it already reached the ring/snapshot like any ordinary byte.
        let chunk = b"\x1b[5A";
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn multiple_queries_are_redelivered_in_original_order() {
        let chunk = b"\x1b[6nfoo\x1b[c";
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b[6n\x1b[c");
    }

    // ---- FR9: OSC color queries ----

    #[test]
    fn osc_color_query_outside_ring_written_ranges_is_redelivered() {
        // Alt-screen span: nothing written toward the ring at all.
        let chunk = b"\x1b]10;?\x07";
        let result = build_suppressed_replacement(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_color_query_inside_ring_written_ranges_is_not_redelivered() {
        let chunk = b"\x1b]11;?\x07";
        // Main-buffer span: written toward the ring, so it already survives
        // in the snapshot byte-for-byte (D7) — must not be duplicated.
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn osc_4_indexed_color_query_is_recognized() {
        let chunk = b"\x1b]4;5;?\x07";
        let result = build_suppressed_replacement(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_color_query_terminated_by_st_is_recognized() {
        let chunk = b"\x1b]12;?\x1b\\";
        let result = build_suppressed_replacement(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_non_color_query_is_not_redelivered() {
        // OSC 2 (title set) is not a color query; never re-delivered.
        let chunk = b"\x1b]2;my title\x07";
        let result = build_suppressed_replacement(chunk, &[], &[]);
        assert!(result.is_empty());
    }

    // ---- FR10: cut tails ----

    #[test]
    fn cut_csi_tail_is_redelivered_verbatim() {
        let chunk = b"plain\x1b[3";
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b[3");
    }

    #[test]
    fn bare_trailing_esc_is_treated_as_a_tail() {
        let chunk = b"plain\x1b";
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b");
    }

    #[test]
    fn cut_utf8_character_tail_is_redelivered_verbatim() {
        // U+4E2D "中" is E4 B8 AD; keep only the first two bytes.
        let mut chunk = b"plain".to_vec();
        chunk.extend_from_slice(&[0xe4, 0xb8]);
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, vec![0xe4, 0xb8]);
    }

    #[test]
    fn complete_utf8_character_is_not_treated_as_a_tail() {
        let chunk = "plain中".as_bytes();
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn pending_rich_content_candidate_is_redelivered_as_the_whole_pending_run() {
        // The write filter is already holding an unterminated OSC 9999
        // introducer; D8 says T is the WHOLE pending run, verbatim. The
        // query lives in this chunk's ALT-SCREEN portion — `pending` only
        // ever draws from main-buffer (`ring_written_ranges`) content, so
        // an empty `ring_written_ranges` here means the query is entirely
        // unrelated to (and never excluded by) the pending run.
        let pending = b"\x1b]9999;emterm-md;partial".to_vec();
        let chunk = b"\x1b[6n"; // a query earlier in the same read, unrelated to pending
        let result = build_suppressed_replacement(chunk, &[], &pending);
        let mut expected = b"\x1b[6n".to_vec();
        expected.extend_from_slice(&pending);
        assert_eq!(result, expected);
    }

    #[test]
    fn pending_run_excludes_its_own_contribution_from_the_query_scan() {
        // The whole chunk's main-buffer content was swallowed into pending
        // (nothing new resolved this read) — no query scan should run over
        // any of it, even though it happens to contain query-shaped bytes,
        // because none of it has been classified as complete yet.
        let chunk = b"\x1b[6n";
        let pending = chunk.to_vec();
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &pending);
        assert_eq!(
            result, pending,
            "must not also extract the pending bytes as a query"
        );
    }

    // ---- TM-1: never fabricate a query from inside a string payload ----

    #[test]
    fn embedded_esc_aborts_the_osc_and_the_following_csi_query_is_extracted() {
        // A bare ESC inside an OSC string payload ABORTS the OSC (matching
        // the crate's own `write_filter::find_osc_end` convention: an OSC
        // body never tolerates an embedded bare ESC). Scanning resumes at
        // the abort point as a fresh top-level position, so the CSI device
        // query that follows is a genuine, un-nested query and IS
        // extracted — it was never actually inside a valid OSC string.
        let chunk = b"\x1b]2;title with \x1b[6n inside\x07";
        let result = build_suppressed_replacement(chunk, &[], &[]);
        assert_eq!(result, b"\x1b[6n");
    }

    #[test]
    fn query_shaped_bytes_inside_dcs_string_payload_are_not_extracted() {
        let mut chunk = b"\x1bPq".to_vec();
        chunk.extend_from_slice(b"\x1b[6n"); // query-shaped bytes inside the DCS body
        chunk.extend_from_slice(b"\x1b\\");
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn query_shaped_bytes_inside_apc_string_payload_are_not_extracted() {
        let mut chunk = b"\x1b_G".to_vec();
        chunk.extend_from_slice(b"\x1b]10;?\x07"); // query-shaped bytes inside the APC body
        chunk.extend_from_slice(b"\x1b\\");
        let result = build_suppressed_replacement(&chunk, &[], &[]);
        assert!(result.is_empty());
    }

    // ---- TM-2: bounded / adversarial ----

    #[test]
    fn adversarial_repeated_introducers_complete_within_a_time_bound() {
        let chunk = vec![0x1b, b'['].repeat(20_000);
        let start = std::time::Instant::now();
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[]);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "adversarial input must not blow up the scan cost"
        );
        assert!(result.len() <= chunk.len());
    }

    #[test]
    fn result_never_exceeds_chunk_length_when_pending_is_empty() {
        let chunk = b"\x1b[6nsome text\x1b[c\x1b[3";
        let result = build_suppressed_replacement(chunk, &[0..chunk.len()], &[]);
        assert!(result.len() <= chunk.len());
    }
}
