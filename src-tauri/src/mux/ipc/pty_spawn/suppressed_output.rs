//! Replacement payload builder for a suppressed chunk (mux-snapshot-output-boundary
//! task0001, IMPLEMENTATION.md Shared Components "Replacement payload for a
//! suppressed chunk"; rebuilt on the client-parity classifier by
//! mux-suppressed-output-fixes task0001).
//!
//! When the PTY reader suppresses a chunk for a destination that a delivered
//! snapshot already covers (FR3/FR11 of mux-snapshot-output-boundary), two
//! guarantees still have to hold for that destination:
//!
//! - **FR9** (now FR7/FR9 of mux-suppressed-output-fixes): any complete
//!   query (CSI device query or OSC color query) or viewer launch inside the
//!   suppressed chunk that does NOT survive into the snapshot in a way the
//!   client would still act on must still reach the client once, so a
//!   program blocked on that query's response is not left waiting forever,
//!   and a viewer window still opens.
//! - **FR10** (now FR1/FR3/FR6): an incomplete control sequence or UTF-8
//!   character cut off at the suppressed chunk's boundary must not have its
//!   continuation (in a later, delivered chunk) misread as literal text or a
//!   stray replacement character — including when the cut sequence started
//!   in the read BEFORE this suppressed chunk (the reader's retained
//!   window, FR6).
//!
//! - **FR8** (mux-suppressed-output-round2-fixes task0004): when the
//!   destination's covering snapshot already left the client parser holding
//!   the very tail this builder would append (a cut UTF-8 character,
//!   `ESC (` / `ESC )`, or a cut CSI), that tail is not sent a second time
//!   — see [`SuppressedReplacementRequest::snapshot_trailing_construct`].
//!
//! [`build_suppressed_replacement`] answers both by handing `window` (the
//! stream bytes before this chunk) and `chunk` to
//! [`super::client_parity_scan::scan`], which walks them as one continuous
//! buffer the same way the client's `term_core` parser does, and then
//! assembles `Q ++ T`: `Q` is every qualifying item in stream order, `T` is
//! the tail (either the write filter's still-pending run, per D4, or the
//! classifier's own discovered incomplete trailing construct). The result is
//! delivered as an ordinary `PtyOutput` chunk to the suppressed chunk's
//! destination, after the snapshot and before the reader's next chunk — see
//! `pty_reader_loop`'s call site.
//!
//! TM-1 (never fabricate a query): the classifier only starts a construct
//! where the client's parser would. TM-2 (bounded, never empty): the scan is
//! a single forward pass over `window.len() + chunk.len()`, this function
//! does no work at all for a chunk that was not suppressed (callers only
//! invoke it for suppressed chunks), and an empty result is never turned
//! into an empty `PtyOutput` chunk by the caller (that would read as PTY
//! exit).

use std::ops::Range;

use super::client_parity_scan::{
    self, ScanItemKind, ScanOutcome, is_color_query, is_viewer_launch,
    reconstruct_osc_number_and_data,
};
use crate::mux::snapshot_tail::is_awaiting_designator;

/// The ASCII charset designator byte sent ahead of the items when the client
/// is waiting for a designator (FR8).
const FILLER_DESIGNATOR: u8 = b'B';

/// A sequence that was held in the write filter's pending run before this
/// read and reached its terminator inside this read (mux-suppressed-output-
/// round2-fixes FR6), mapped to chunk coordinates by the reader.
#[derive(Debug, Clone, Copy)]
pub(in crate::mux) struct CarriedOverCompletion<'a> {
    /// The complete sequence bytes, from its opening `ESC` through its
    /// terminator.
    pub(in crate::mux) bytes: &'a [u8],
    /// Position just past the terminator, in chunk coordinates.
    pub(in crate::mux) end: usize,
}

/// Build the FR1/FR3/FR6/FR7/FR9 replacement payload for a suppressed chunk.
///
/// - `chunk`: the suppressed chunk's raw bytes (before any stripping).
/// - `ring_written_ranges`: the byte ranges of `chunk` that were fed toward
///   the scrollback ring this read (the reader's `live_spans` /
///   `main_spans` — main-buffer content; alternate-screen spans are NOT
///   included). Used to decide whether the chunk's last byte lies in a
///   ring-written span and how much of the write filter's pending run this
///   chunk contributed (D4). It does NOT decide which items are delivered:
///   a color query, like a CSI query, is delivered whether or not its bytes
///   survived into the ring (round-2 FR2), because the snapshot is replayed
///   with its responses discarded.
/// - `pending_after`: [`super::write_filter::ScrollbackWriteFilter::pending`]'s
///   contents taken right after this chunk was fed to the filter. Never
///   empty unless nothing is currently held back.
/// - `window`: the stream bytes immediately preceding `chunk` (the reader's
///   retained window, FR6, up to
///   [`client_parity_scan::RETAINED_WINDOW_BYTES`]), as it stood BEFORE this
///   chunk arrived.
///
/// Returns `Q ++ T`, which may be empty (see the module doc's TM-2 note —
/// an empty result must never itself become an empty `PtyOutput` chunk).
///
/// No snapshot trailing construct and no carried-over completion are known
/// here (neither the FR8 rule nor the FR6 delivery applies); use
/// [`build_suppressed_replacement_for`] to supply them.
#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::mux) fn build_suppressed_replacement(
    chunk: &[u8],
    ring_written_ranges: &[Range<usize>],
    pending_after: &[u8],
    window: &[u8],
) -> Vec<u8> {
    build_suppressed_replacement_for(&SuppressedReplacementRequest {
        chunk,
        ring_written_ranges,
        pending_after,
        window,
        snapshot_trailing_construct: None,
        carried_over_completion: None,
    })
}

/// One value carrying every input of replacement assembly for one
/// suppressed chunk (mux-suppressed-output-round2-fixes IMPLEMENTATION.md
/// Shared Components, D2). The four base fields are exactly the parameters
/// of [`build_suppressed_replacement`]; the trailing fields are owned by the
/// tasks that introduced them.
pub(in crate::mux) struct SuppressedReplacementRequest<'a> {
    /// The suppressed chunk's raw bytes.
    pub chunk: &'a [u8],
    /// The ring-written ranges of `chunk`, ascending, in chunk coordinates.
    pub ring_written_ranges: &'a [Range<usize>],
    /// The write filter's pending bytes right after this read.
    pub pending_after: &'a [u8],
    /// The retained window preceding `chunk`.
    pub window: &'a [u8],
    /// Snapshot trailing construct (task0004, FR8): absent, or the bytes of
    /// the incomplete construct the destination's covering snapshot left the
    /// client parser in. Applies only to a tail found by the client-parity
    /// scan, never to the write filter's pending-run tail.
    pub snapshot_trailing_construct: Option<&'a [u8]>,
    /// Carried-over completion (task0003, FR6): absent, or the sequence that
    /// was held in the write filter's pending run before this read and
    /// completed inside it, with its end position in chunk coordinates.
    pub carried_over_completion: Option<CarriedOverCompletion<'a>>,
}

/// [`build_suppressed_replacement`] over a full request: the result of
/// [`finalize_suppressed_replacement`] applied to
/// [`prepare_suppressed_replacement`], with the request's own snapshot
/// trailing construct. Signature and result are unchanged for every input
/// (mux-suppressed-output-round3-fixes task0004, FR6: the split exists so the
/// reader can decide tail omission from the destination's record as it stands
/// at send time).
#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::mux) fn build_suppressed_replacement_for(
    request: &SuppressedReplacementRequest<'_>,
) -> Vec<u8> {
    finalize_suppressed_replacement(
        prepare_suppressed_replacement(request),
        request.snapshot_trailing_construct,
    )
}

/// A suppressed chunk's replacement assembled up to, but not including, the
/// tail-omission decision (mux-suppressed-output-round3-fixes task0004, FR6).
/// Owned, so it can be held while the reader secures a send slot and re-takes
/// its exclusions.
pub(in crate::mux) struct PreparedReplacement {
    /// The assembled items: every reportable query / launch, in stream order,
    /// the carried-over completion included.
    items: Vec<u8>,
    /// The tail bytes: the write filter's pending run (D4 rule 1) or the
    /// client-parity scan's own incomplete trailing construct.
    tail: Vec<u8>,
    /// Whether the tail may be omitted when the destination's snapshot
    /// already left the client holding it. Only a tail found by the
    /// client-parity scan may; the write filter's pending run never may.
    tail_omittable: bool,
}

impl PreparedReplacement {
    /// Neither items nor a tail: no construct can make the finalized
    /// replacement non-empty.
    pub(in crate::mux) fn is_empty(&self) -> bool {
        self.items.is_empty() && self.tail.is_empty()
    }
}

/// The part of replacement assembly that needs no knowledge of the
/// destination's snapshot: the client-parity scan, the item assembly
/// (carried-over completion included) and the tail selection (FR6).
///
/// `request.snapshot_trailing_construct` is NOT read here — it is the input of
/// [`finalize_suppressed_replacement`]. The reader passes `None`. Runs outside
/// every exclusion; the cost is the scan, as before.
pub(in crate::mux) fn prepare_suppressed_replacement(
    request: &SuppressedReplacementRequest<'_>,
) -> PreparedReplacement {
    let SuppressedReplacementRequest {
        chunk,
        ring_written_ranges,
        pending_after,
        window,
        snapshot_trailing_construct: _,
        carried_over_completion,
    } = *request;
    if chunk.is_empty() {
        return PreparedReplacement {
            items: Vec::new(),
            tail: Vec::new(),
            tail_omittable: false,
        };
    }

    // D4: the write filter's pending run is the tail ONLY when it is
    // non-empty AND the chunk's last byte lies in a ring-written (main-
    // buffer) span. When the chunk ends outside ring-written spans (the
    // alternate screen), the client's own parser already saw the
    // screen-switch ESC and aborted whatever pending tracks — re-sending it
    // would duplicate/misrepresent a sequence the client already closed, so
    // the classifier's own tail (over the FULL chunk) is used instead.
    let chunk_ends_in_ring_written = ring_written_ranges
        .last()
        .is_some_and(|r| r.end == chunk.len());

    let (mut items, tail, tail_omittable) =
        if !pending_after.is_empty() && chunk_ends_in_ring_written {
            // D4 rule 1 / FR5: nothing is extracted from the chunk bytes pending
            // will re-deliver (TM-1). Those are exactly the last
            // min(pending, fed) bytes of the concatenated ring-written ranges;
            // exclude the chunk-coordinate pieces that make them up — never the
            // gaps (alternate-screen ranges, removed switch sequences) between
            // ranges — and walk the whole chunk so items around them survive.
            let fed_len: usize = ring_written_ranges
                .iter()
                .map(|r| r.end.saturating_sub(r.start))
                .sum();
            let contribution = pending_after.len().min(fed_len);
            let excluded = excluded_pieces(ring_written_ranges, contribution);
            let outcome = client_parity_scan::scan(window, chunk, &excluded);
            let items = assemble_items(&outcome, carried_over_completion.as_ref());
            (items, pending_after.to_vec(), false)
        } else {
            let outcome = client_parity_scan::scan(window, chunk, &[]);
            let items = assemble_items(&outcome, carried_over_completion.as_ref());
            let tail: Vec<u8> = match outcome.tail.clone() {
                Some(tail_range) => {
                    let tail_bytes = &outcome.combined[tail_range];
                    if outcome.tail_strip_c0 {
                        strip_c0(tail_bytes).collect()
                    } else {
                        tail_bytes.to_vec()
                    }
                }
                None => Vec::new(),
            };
            (items, tail, true)
        };
    // The finalize step (which may run under the reader's exclusions) then
    // only compares and appends; make the append a copy into spare capacity.
    if !items.is_empty() {
        items.reserve(tail.len());
    }
    PreparedReplacement {
        items,
        tail,
        tail_omittable,
    }
}

/// The tail-omission decision and the final concatenation (FR6). Performs no
/// scan, no identification and no decider call — it only compares and
/// concatenates, so the reader may run it under its exclusions.
///
/// FR8: when the tail is omittable, non-empty and equal to `construct`, the
/// destination's covering snapshot left the client parser holding exactly this
/// tail. Re-sending it would double the construct (a second `ESC (` would be
/// taken as the designator and printed), so it is treated as already carried —
/// see `carried_tail_output`. In every other case the result is the items
/// followed by the tail.
pub(in crate::mux) fn finalize_suppressed_replacement(
    prepared: PreparedReplacement,
    construct: Option<&[u8]>,
) -> Vec<u8> {
    let PreparedReplacement {
        mut items,
        tail,
        tail_omittable,
    } = prepared;
    let already_carried =
        tail_omittable && !tail.is_empty() && construct.is_some_and(|c| c == tail.as_slice());
    if already_carried {
        return carried_tail_output(items, tail);
    }
    if items.is_empty() {
        return tail;
    }
    items.extend_from_slice(&tail);
    items
}

/// The replacement for a chunk whose tail the client already holds (FR8):
/// `items` is the assembled query / launch output, `tail` the tail the
/// snapshot carried.
///
/// - no items: nothing is sent (the caller never turns an empty result into
///   an empty `PtyOutput` chunk);
/// - items and an awaiting-designator tail: one filler designator byte, the
///   items, then the tail. The client's pending designator slot absorbs the
///   filler, the items parse intact, and the re-sent `ESC (` / `ESC )`
///   restores the slot for the next chunk's first byte;
/// - items and a UTF-8 or CSI tail: the items, then the tail. The items'
///   leading ESC silently ends a UTF-8 partial and aborts a CSI, and the tail
///   restores the state.
fn carried_tail_output(items: Vec<u8>, tail: Vec<u8>) -> Vec<u8> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(1 + items.len() + tail.len());
    if is_awaiting_designator(&tail) {
        out.push(FILLER_DESIGNATOR);
    }
    out.extend_from_slice(&items);
    out.extend_from_slice(&tail);
    out
}

/// One reportable item, keyed by where it ends in chunk coordinates.
struct EmittedItem {
    /// Position just past the item's last byte, in chunk coordinates.
    chunk_end: usize,
    bytes: Vec<u8>,
}

/// Assemble the byte output for every reportable item, in raw-stream order
/// (ascending chunk end position):
/// - CSI device queries: from any region, C0 control bytes removed (FR3) —
///   their effect already took place via the snapshot.
/// - Color queries: from any region — inside a ring-written range, outside
///   it, or across a range edge — the whole OSC, terminator included,
///   verbatim (round-2 FR2). The snapshot is replayed with its responses
///   discarded, so a query that survived into the ring is never answered
///   through the snapshot; this is the same treatment CSI queries get.
/// - Viewer launches: from any region, the whole sequence verbatim.
/// - The carried-over completion (FR6), when it is a color query or a
///   deliverable viewer launch, verbatim — unless a scan item ends at the
///   same chunk position: that is the same sequence, re-detected through the
///   retained window, and is emitted once. Items with identical content but
///   different end positions are distinct and all emitted.
fn assemble_items(outcome: &ScanOutcome, carried: Option<&CarriedOverCompletion<'_>>) -> Vec<u8> {
    let mut items: Vec<EmittedItem> = Vec::new();
    for item in &outcome.items {
        let chunk_end = item.range.end - outcome.boundary;
        let bytes = match item.kind {
            ScanItemKind::ColorQuery | ScanItemKind::ViewerLaunch => {
                outcome.combined[item.range.clone()].to_vec()
            }
            ScanItemKind::CsiQuery => strip_c0(&outcome.combined[item.range.clone()]).collect(),
        };
        items.push(EmittedItem { chunk_end, bytes });
    }

    if let Some(carried) = carried
        && is_deliverable_osc(carried.bytes)
        && !items.iter().any(|i| i.chunk_end == carried.end)
    {
        items.push(EmittedItem {
            chunk_end: carried.end,
            bytes: carried.bytes.to_vec(),
        });
        // Scan items are already ascending; a stable sort places the
        // carried item by its end position and keeps ties in scan order.
        items.sort_by_key(|i| i.chunk_end);
    }

    let mut out = Vec::new();
    for item in items {
        out.extend_from_slice(&item.bytes);
    }
    out
}

/// Whether a carried-over sequence is one this builder delivers: a complete
/// OSC (`ESC ] body BEL|ST`) that is a color query or a deliverable viewer
/// launch. DCS/APC sequences and every other OSC are not — the body is
/// recovered through the same delivery-side entry points the scan uses.
fn is_deliverable_osc(bytes: &[u8]) -> bool {
    if bytes.len() < 3 || bytes[0] != 0x1b || bytes[1] != b']' {
        return false;
    }
    let last = bytes.len() - 1;
    let body_end = if bytes[last] == 0x07 {
        last
    } else if bytes[last] == b'\\' && bytes.len() >= 4 && bytes[last - 1] == 0x1b {
        last - 1
    } else {
        return false;
    };
    let osc = reconstruct_osc_number_and_data(&bytes[2..body_end]);
    is_color_query(&osc) || is_viewer_launch(&osc)
}

/// Filter out C0 control bytes (0x00-0x1A, 0x1C-0x1F) from `bytes` — never
/// overlaps a valid CSI param, intermediate, private-marker or final byte,
/// so a value-based filter cannot drop anything else (FR3).
fn strip_c0(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    bytes
        .iter()
        .copied()
        .filter(|&c| !matches!(c, 0x00..=0x1a | 0x1c..=0x1f))
}

/// Given `ranges` (ascending, non-overlapping byte ranges of some buffer)
/// whose concatenation is the fed stream, return — in the ORIGINAL buffer's
/// coordinates, ascending — the pieces that make up the trailing
/// `contribution` bytes of that concatenation (FR5). `contribution` is
/// clamped by the caller to the ranges' total length; a smaller total simply
/// yields every non-empty range. Empty ranges contribute no piece.
fn excluded_pieces(ranges: &[Range<usize>], contribution: usize) -> Vec<Range<usize>> {
    let mut remaining = contribution;
    let mut pieces = Vec::new();
    for r in ranges.iter().rev() {
        if remaining == 0 {
            break;
        }
        let len = r.end.saturating_sub(r.start);
        if len == 0 {
            continue;
        }
        let take = len.min(remaining);
        pieces.push(r.end - take..r.end);
        remaining -= take;
    }
    pieces.reverse();
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(chunk: &[u8], ring_written_ranges: &[Range<usize>], pending: &[u8]) -> Vec<u8> {
        build_suppressed_replacement(chunk, ring_written_ranges, pending, &[])
    }

    #[test]
    fn empty_chunk_yields_empty_result() {
        assert!(build(&[], &[], &[]).is_empty());
    }

    #[test]
    fn plain_output_with_no_query_and_no_tail_yields_empty_result() {
        let chunk = b"hello, world\r\n";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert!(
            result.is_empty(),
            "no query, no tail => nothing to re-deliver"
        );
    }

    // ---- FR9: CSI device queries ----

    #[test]
    fn csi_cursor_position_query_is_redelivered() {
        let chunk = b"\x1b[6n";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn csi_primary_device_attributes_query_is_redelivered() {
        let chunk = b"\x1b[c";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn csi_device_query_surrounded_by_plain_text_is_extracted_alone() {
        let chunk = b"before\x1b[6nafter";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b[6n");
    }

    #[test]
    fn non_query_csi_is_not_redelivered() {
        // A complete, non-query CSI (cursor-up) must not be re-delivered —
        // it already reached the ring/snapshot like any ordinary byte.
        let chunk = b"\x1b[5A";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn multiple_queries_are_redelivered_in_original_order() {
        let chunk = b"\x1b[6nfoo\x1b[c";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b[6n\x1b[c");
    }

    // ---- FR9: OSC color queries ----

    #[test]
    fn osc_color_query_outside_ring_written_ranges_is_redelivered() {
        // Alt-screen span: nothing written toward the ring at all.
        let chunk = b"\x1b]10;?\x07";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_color_query_inside_ring_written_ranges_is_redelivered_once() {
        // Round-2 FR2 (behavior changed on purpose, SPEC AC-8): a main-buffer
        // span is written toward the ring, but the snapshot is replayed with
        // its responses discarded, so a color query that survived into the
        // ring is never answered through the snapshot. The replacement
        // delivers it once, exactly like a CSI query.
        let chunk = b"\x1b]11;?\x07";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_4_indexed_color_query_is_recognized() {
        let chunk = b"\x1b]4;5;?\x07";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_color_query_terminated_by_st_is_recognized() {
        let chunk = b"\x1b]12;?\x1b\\";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_non_color_query_is_not_redelivered() {
        // OSC 2 (title set) is not a color query; never re-delivered.
        let chunk = b"\x1b]2;my title\x07";
        let result = build(chunk, &[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn osc_10_chained_query_element_is_recognized() {
        // "?;?;?" answers fg/bg/cursor-fg in one dispatch — still a single
        // qualifying OSC, delivered whole.
        let chunk = b"\x1b]10;?;?;?\x07";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn osc_10_set_then_query_mixed_payload_is_recognized_as04() {
        // as-04: a payload mixing a SET element with a QUERY element still
        // counts as a query and is delivered whole.
        let chunk = b"\x1b]10;#fff;?\x07";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    // ---- FR10: cut tails ----

    #[test]
    fn cut_csi_tail_is_redelivered_verbatim() {
        let chunk = b"plain\x1b[3";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b[3");
    }

    #[test]
    fn bare_trailing_esc_is_treated_as_a_tail() {
        // AC-8: unchanged from before this feature — a bare ESC with no
        // GROUND-level string context is a tail regardless of FR1 (FR1 is
        // specifically about an ESC INSIDE an OSC/DCS/APC string body).
        let chunk = b"plain\x1b";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b");
    }

    #[test]
    fn cut_utf8_character_tail_is_redelivered_verbatim() {
        // U+4E2D "中" is E4 B8 AD; keep only the first two bytes.
        let mut chunk = b"plain".to_vec();
        chunk.extend_from_slice(&[0xe4, 0xb8]);
        let result = build(&chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, vec![0xe4, 0xb8]);
    }

    #[test]
    fn complete_utf8_character_is_not_treated_as_a_tail() {
        let chunk = "plain中".as_bytes();
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn pending_rich_content_candidate_is_redelivered_as_the_whole_pending_run() {
        // AC-8 (behavior changed by D4's ring-written-end gate): a
        // non-empty pending run is only used as the tail when the chunk's
        // OWN last byte lies in a ring-written span. Here the chunk is
        // entirely outside ring-written spans (alt-screen), so per D4 rule
        // 3 the pending run — which the client already implicitly aborted
        // by switching screens — is NOT delivered; the classifier's own
        // scan of the chunk runs instead, extracting the CSI query it
        // actually contains.
        let pending = b"\x1b]9999;emterm-md;partial".to_vec();
        let chunk = b"\x1b[6n";
        let result = build(chunk, &[], &pending);
        assert_eq!(
            result, b"\x1b[6n",
            "chunk ends outside ring-written spans: pending must not be re-sent"
        );
    }

    #[test]
    fn pending_run_excludes_its_own_contribution_from_the_query_scan() {
        // The whole chunk's main-buffer content was swallowed into pending
        // (nothing new resolved this read) and the chunk ends IN a
        // ring-written span, so D4 rule 1 applies: pending is the tail, and
        // no query scan should run over any of it, even though it happens
        // to contain query-shaped bytes, because none of it has been
        // classified as complete yet.
        let chunk = b"\x1b[6n";
        let pending = chunk.to_vec();
        let result = build(chunk, &[0..chunk.len()], &pending);
        assert_eq!(
            result, pending,
            "must not also extract the pending bytes as a query"
        );
    }

    #[test]
    fn pending_does_not_exclude_a_query_in_an_unrelated_alt_region() {
        // D4 rule 1's exclusion window is anchored to the CHUNK'S OWN
        // ring-written suffix. A query earlier in the chunk, in an
        // alt-screen span untouched by ring writes, is unaffected by an
        // unrelated pending run that the write filter is holding from a
        // ring-written suffix later in the SAME chunk.
        let mut chunk = b"\x1b[6n".to_vec();
        chunk.extend_from_slice(b"\x1b]9999;emterm-md;partial");
        let ring_written_ranges = vec![4..chunk.len()];
        let pending = b"\x1b]9999;emterm-md;partial".to_vec();
        let result = build(&chunk, &ring_written_ranges, &pending);
        let mut expected = b"\x1b[6n".to_vec();
        expected.extend_from_slice(&pending);
        assert_eq!(result, expected);
    }

    // ---- TM-1: never fabricate a query from inside a string payload ----

    #[test]
    fn embedded_esc_aborts_the_osc_and_the_following_csi_query_is_extracted() {
        // A bare ESC inside an OSC string payload, NOT at the chunk's end,
        // ABORTS the OSC (FR2 (a)): the byte after it starts a fresh
        // escape. The CSI device query that follows is a genuine, un-nested
        // query and IS extracted.
        let chunk = b"\x1b]2;title with \x1b[6n inside\x07";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, b"\x1b[6n");
    }

    #[test]
    fn embedded_esc_at_chunk_end_leaves_the_osc_incomplete_not_aborted() {
        // AC-8 / FR1 (behavior changed): when the embedded ESC is the LAST
        // byte available, the string is INCOMPLETE, not aborted — held as a
        // tail from the OSC's own start, not treated as a completed abort
        // that lets scanning resume past it.
        let chunk = b"\x1b]2;title with \x1b";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, chunk.as_slice());
    }

    #[test]
    fn embedded_esc_inside_dcs_string_payload_aborts_it_and_the_following_csi_query_is_extracted() {
        // as-03: the ESC embedded in the DCS body is not swallowed as
        // payload — it aborts the DCS string (FR2 (b)) and is reprocessed as
        // a fresh escape, so "[6n" right after it is a genuine, newly
        // dispatched CSI device query, not DCS payload (TM-1 only protects
        // bytes that stayed inside a string that actually completed).
        let mut chunk = b"\x1bPq".to_vec();
        chunk.extend_from_slice(b"\x1b[6n");
        chunk.extend_from_slice(b"\x1b\\");
        let result = build(&chunk, &[0..chunk.len()], &[]);
        assert_eq!(result, b"\x1b[6n");
    }

    #[test]
    fn embedded_esc_inside_apc_string_payload_aborts_it_and_the_following_color_query_is_extracted()
    {
        // as-03, APC counterpart: the embedded ESC aborts the APC string and
        // is reprocessed as a fresh escape, so "]10;?\x07" right after it is
        // a genuine, newly dispatched OSC 10 color query, not APC payload.
        let mut chunk = b"\x1b_G".to_vec();
        chunk.extend_from_slice(b"\x1b]10;?\x07");
        chunk.extend_from_slice(b"\x1b\\");
        let result = build(&chunk, &[], &[]);
        assert_eq!(result, b"\x1b]10;?\x07");
    }

    #[test]
    fn replacement_never_extracts_a_query_consumed_as_charset_designator() {
        // AC-1 (TS-2): "ESC ( ESC [ 6 n" — the designator-consuming ESC (
        // swallows the SECOND ESC (even though it's itself 0x1B) as the
        // designator byte (FR2 (d)), so "[ 6 n" is plain ground text, never
        // a CSI query.
        let chunk = b"\x1b(\x1b[6n";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert!(
            result.is_empty(),
            "the CSI query must never be extracted: it was consumed as a \
             charset designator, not seen as a fresh escape by the client"
        );
    }

    // ---- FR7/D3: viewer launches ----

    #[test]
    fn viewer_launch_markdown_is_redelivered() {
        let chunk = b"\x1b]777;emterm;markdown;begin;id=1\x07";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn viewer_launch_json_yaml_html_are_redelivered() {
        for kind in ["json", "yaml", "html"] {
            let chunk = format!("\x1b]777;emterm;{kind};begin;id=1\x07").into_bytes();
            let result = build(&chunk, &[], &[]);
            assert_eq!(result, chunk, "kind={kind}");
        }
    }

    #[test]
    fn viewer_launch_osc_9999_emterm_md_is_redelivered() {
        let chunk = b"\x1b]9999;emterm-md;chunk-data\x07";
        let result = build(chunk, &[], &[]);
        assert_eq!(result, chunk);
    }

    #[test]
    fn viewer_launch_image_kind_is_not_redelivered_d3() {
        // D3 finding: the GUI's `image` kind is a reserved, not-yet-wired
        // no-op (`viewer/mod.rs`'s `"image"` arm) — it never opens a
        // window, so it is not a viewer launch.
        let chunk = b"\x1b]777;emterm;image;begin;id=1\x07";
        let result = build(chunk, &[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn agent_status_kind_is_not_a_viewer_launch() {
        let chunk = b"\x1b]777;emterm;agent-status;idle\x07";
        let result = build(chunk, &[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn inline_images_in_suppressed_chunk_are_not_delivered() {
        // Kitty APC — an inline image, not a viewer launch (known gap,
        // FR7).
        let mut chunk = b"\x1b_Gf=100,a=T;".to_vec();
        chunk.extend_from_slice(b"base64data");
        chunk.extend_from_slice(b"\x1b\\");
        let result = build(&chunk, &[], &[]);
        assert!(result.is_empty());
    }

    // ---- TM-2: bounded / adversarial ----

    #[test]
    fn adversarial_repeated_introducers_complete_within_a_time_bound() {
        let chunk = vec![0x1b, b'['].repeat(20_000);
        let start = std::time::Instant::now();
        let result = build(&chunk, &[0..chunk.len()], &[]);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "adversarial input must not blow up the scan cost"
        );
        assert!(result.len() <= chunk.len());
    }

    #[test]
    fn hostile_chunk_replacement_is_linear_and_never_empty() {
        // AC-7 (TM-1, TM-2, NFR4, NFR5): a 64 KiB hostile chunk (repeated
        // unterminated introducers, a long OSC number, long runs of partial
        // sequences) never panics, never stalls (every scan step makes
        // forward progress — the loop position is never left unchanged, so
        // it never gets stuck at an "empty" step), and completes within a
        // time bound proportional to its length.
        let mut chunk = Vec::with_capacity(64 * 1024);
        while chunk.len() < 64 * 1024 {
            chunk.extend_from_slice(b"\x1b]4444444444444444444444444444444444444444;");
            chunk.extend_from_slice(&[0x1b, b'[']);
            chunk.extend_from_slice(&[0xe4, 0xb8]);
        }
        chunk.truncate(64 * 1024);
        let start = std::time::Instant::now();
        let result = build(&chunk, &[0..chunk.len()], &[]);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "hostile input must not blow up the scan cost"
        );
        assert!(result.len() <= chunk.len());
    }

    #[test]
    fn result_never_exceeds_chunk_length_when_pending_is_empty() {
        let chunk = b"\x1b[6nsome text\x1b[c\x1b[3";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert!(result.len() <= chunk.len());
    }

    // ---- FR6: retained window (start-state derivation) ----

    #[test]
    fn query_split_across_window_and_chunk_is_answered_once() {
        let window = b"prefix text \x1b[6".to_vec();
        let chunk = b"n".to_vec();
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
        assert_eq!(result, b"\x1b[6n");
    }

    #[test]
    fn utf8_character_split_across_window_and_chunk_is_not_redelivered_when_complete() {
        // The character completes across the window/chunk boundary — its
        // bytes are already captured into the ring by BOTH reads (ring
        // capture is unconditional), so nothing needs to be re-delivered.
        let window = b"plain\xe4\xb8".to_vec();
        let chunk = b"\xad more".to_vec();
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
        assert!(result.is_empty());
    }

    #[test]
    fn incomplete_sequence_started_in_window_still_incomplete_at_chunk_end_is_resent_from_its_start()
     {
        let window = b"plain\x1b[3".to_vec();
        let chunk = b"1".to_vec(); // still no final byte
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
        assert_eq!(result, b"\x1b[31");
    }

    #[test]
    fn consecutive_suppressed_chunks_leave_next_forwarded_chunk_intact() {
        // First suppressed chunk ends in an incomplete SGR (not a query).
        let window0 = b"".to_vec();
        let chunk1 = b"\x1b[31".to_vec();
        let result1 = build_suppressed_replacement(&chunk1, &[0..chunk1.len()], &[], &window0);
        assert_eq!(
            result1, b"\x1b[31",
            "chunk1's own incomplete tail is resent"
        );

        // Second suppressed chunk completes the SGR with its final byte.
        // Not a query, so nothing is re-delivered — its display effect
        // already reached the client via the ring/snapshot.
        let window1 = super::super::advance_retained_window(&window0, &chunk1);
        let chunk2 = b"m".to_vec();
        let result2 = build_suppressed_replacement(&chunk2, &[0..chunk2.len()], &[], &window1);
        assert!(
            result2.is_empty(),
            "a completed non-query CSI must not be re-delivered"
        );

        // The classifier never reaches past chunk2's own bytes, so a
        // subsequent forwarded chunk's first byte cannot be consumed as
        // part of any CSI the classifier tracked.
        let window2 = super::super::advance_retained_window(&window1, &chunk2);
        let forwarded_next = b"X".to_vec();
        // Not suppressed in production (bypasses this builder entirely);
        // simulate a hypothetical suppression to prove no leakage.
        let result3 = build_suppressed_replacement(
            &forwarded_next,
            &[0..forwarded_next.len()],
            &[],
            &window2,
        );
        assert!(result3.is_empty(), "plain text is never re-delivered");
    }

    // ---- FR6/NFR3: start-state derivation edge cases ----

    #[test]
    fn full_window_with_no_decidable_esc_falls_back_to_ground() {
        let window = vec![b'x'; client_parity_scan::RETAINED_WINDOW_BYTES];
        let chunk = b"\x1b[6n".to_vec();
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
        assert_eq!(result, b"\x1b[6n");
    }

    #[test]
    fn full_window_leading_esc_at_offset_zero_is_excluded_as_undecidable() {
        // TM-1: an ESC at window offset 0 is NEVER a usable start position
        // (its own designator context cannot be decided) — this can only
        // ever cause a MISS, never a fabrication.
        let mut window = vec![b'x'; client_parity_scan::RETAINED_WINDOW_BYTES];
        window[0] = 0x1b;
        let chunk = b"[6n".to_vec();
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
        assert!(result.is_empty());
    }

    #[test]
    fn full_window_with_no_decidable_esc_and_trailing_utf8_partial_is_carried_over() {
        let mut window = vec![b'x'; client_parity_scan::RETAINED_WINDOW_BYTES - 1];
        window.push(0xe4); // 3-byte UTF-8 lead, incomplete
        let chunk = vec![0xb8, 0xad];
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
        assert!(
            result.is_empty(),
            "the character completes across window+chunk; already in ring"
        );
    }

    // ---- AC-4 (FR6, NFR3; TS-6, TS-7): split sweeps and the window itself ----

    #[test]
    fn query_split_across_reads_is_answered_once_at_every_split_position() {
        // For ESC[6n split at EVERY position between the previous read
        // (the window) and a suppressed chunk, the query is answered
        // exactly once — never missed, never duplicated.
        let query = b"\x1b[6n";
        for split in 0..query.len() {
            let window = query[..split].to_vec();
            let chunk = query[split..].to_vec();
            let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
            assert_eq!(
                result, b"\x1b[6n",
                "split at position {split}: the query must be answered exactly once"
            );
        }
    }

    #[test]
    fn utf8_split_across_reads_never_prints_a_replacement_character() {
        // For a 3-byte UTF-8 character split at EVERY position between
        // the window and the chunk, the character already reached the
        // client whole via the ring (both reads are captured
        // unconditionally) — nothing must be redelivered, so no partial
        // byte sequence can ever reach the client and render as U+FFFD.
        let ch = [0xe4u8, 0xb8, 0xad]; // "中"
        for split in 1..ch.len() {
            let window = ch[..split].to_vec();
            let chunk = ch[split..].to_vec();
            let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
            assert!(
                result.is_empty(),
                "split at position {split}: a completed character must not be redelivered"
            );
        }
    }

    #[test]
    fn retained_window_holds_last_n_bytes_across_reads() {
        // AC-4 (NFR3): the retained window equals the last min(N, total)
        // bytes before each read, for reads of varying sizes.
        use super::super::advance_retained_window;
        let n = client_parity_scan::RETAINED_WINDOW_BYTES;

        // Stream shorter than N: the window holds everything so far.
        let w0 = advance_retained_window(&[], b"short");
        assert_eq!(w0, b"short");

        // A read that itself exceeds N: the window becomes exactly its
        // own last N bytes.
        let big = vec![b'a'; n + 100];
        let w1 = advance_retained_window(&w0, &big);
        assert_eq!(w1.len(), n);
        assert_eq!(w1, big[big.len() - n..]);

        // A further small read: the window becomes the last N bytes of
        // window ++ chunk.
        let small = b"tail-bytes".to_vec();
        let w2 = advance_retained_window(&w1, &small);
        assert_eq!(w2.len(), n);
        let mut combined = w1.clone();
        combined.extend_from_slice(&small);
        assert_eq!(w2, combined[combined.len() - n..]);
    }

    // ---- AC-2 (FR3; TS-3) ----

    #[test]
    fn incomplete_csi_tail_is_resent_without_executed_c0_controls() {
        // An incomplete CSI tail containing an LF is resent with the LF
        // removed: the LF already executed (moved the cursor) via the
        // snapshot, so re-sending it verbatim would execute it a second
        // time. Holds both when the CSI starts in the suppressed chunk
        // itself and when it started in the previous read.
        let chunk = b"plain\x1b[3\n5";
        let result = build(chunk, &[0..chunk.len()], &[]);
        assert_eq!(
            result, b"\x1b[35",
            "the LF inside a tail starting in THIS chunk must be stripped"
        );

        let window = b"plain\x1b[3".to_vec();
        let chunk2 = b"\n5".to_vec();
        let result2 = build_suppressed_replacement(&chunk2, &[0..chunk2.len()], &[], &window);
        assert_eq!(
            result2, b"\x1b[35",
            "the LF must be stripped even when the CSI started in the previous read"
        );
    }

    // ---- AC-3 (FR4; TS-4) ----

    #[test]
    fn pending_does_not_drop_alt_region_queries_viewer_launches_or_own_tail() {
        // (a) Pending non-empty, chunk ends IN a ring-written span: an
        // earlier alt-region query survives pending's exclusion zone.
        let query = b"\x1b[6n".to_vec();
        let pending_literal = b"\x1b]9999;emterm-md;partial".to_vec();
        let mut chunk_a = query.clone();
        chunk_a.extend_from_slice(&pending_literal);
        let ring_a = vec![query.len()..chunk_a.len()];
        let result_a = build(&chunk_a, &ring_a, &pending_literal);
        let mut expected_a = query.clone();
        expected_a.extend_from_slice(&pending_literal);
        assert_eq!(result_a, expected_a, "(a) alt-region query survives");

        // (b) Same shape, but the earlier alt-region item is a viewer
        // launch instead of a query.
        let viewer_launch = b"\x1b]777;emterm;markdown;begin;id=1\x07".to_vec();
        let mut chunk_b = viewer_launch.clone();
        chunk_b.extend_from_slice(&pending_literal);
        let ring_b = vec![viewer_launch.len()..chunk_b.len()];
        let result_b = build(&chunk_b, &ring_b, &pending_literal);
        let mut expected_b = viewer_launch.clone();
        expected_b.extend_from_slice(&pending_literal);
        assert_eq!(
            result_b, expected_b,
            "(b) alt-region viewer launch survives"
        );

        // (c) Pending non-empty but the chunk ends OUTSIDE any
        // ring-written span (alt-screen) — D4 rule 3: pending is not
        // re-sent, and the classifier's own tail (a genuinely incomplete
        // CSI at the chunk's end) is delivered instead.
        let chunk_c = b"\x1b[3";
        let result_c = build(chunk_c, &[], &pending_literal);
        assert_eq!(
            result_c, b"\x1b[3",
            "(c) chunk ends outside ring-written spans: pending dropped, own tail resent"
        );
    }

    // ---- AC-5 (FR7; TS-8) ----

    #[test]
    fn viewer_launch_in_suppressed_chunk_is_delivered_exactly_once() {
        // A complete OSC 777 markdown viewer launch is delivered exactly
        // once, in stream order among the other items, and before the
        // tail.
        let mut chunk = b"\x1b[6n".to_vec(); // query, first in stream order
        chunk.extend_from_slice(b"\x1b]777;emterm;markdown;begin;id=1\x07");
        chunk.extend_from_slice(b"\x1b[3"); // own incomplete tail
        let result = build(&chunk, &[0..chunk.len()], &[]);
        let mut expected = b"\x1b[6n".to_vec();
        expected.extend_from_slice(b"\x1b]777;emterm;markdown;begin;id=1\x07");
        expected.extend_from_slice(b"\x1b[3");
        assert_eq!(
            result, expected,
            "query, then viewer launch, then tail — each exactly once, in stream order"
        );

        // Exactly once even when a pending re-send shares the
        // replacement: the viewer launch precedes the ring-written
        // suffix that pending re-sends, and must not be duplicated or
        // dropped.
        let viewer_launch = b"\x1b]777;emterm;markdown;begin;id=1\x07".to_vec();
        let pending = b"\x1b]9999;emterm-md;partial".to_vec();
        let mut chunk2 = viewer_launch.clone();
        chunk2.extend_from_slice(&pending);
        let ring2 = vec![viewer_launch.len()..chunk2.len()];
        let result2 = build(&chunk2, &ring2, &pending);
        let mut expected2 = viewer_launch.clone();
        expected2.extend_from_slice(&pending);
        assert_eq!(
            result2, expected2,
            "the viewer launch is delivered exactly once alongside a pending re-send"
        );
    }

    // ---- AC-1 (FR1, FR2; TS-1, TS-2) ----

    #[test]
    fn suppressed_alt_osc_color_query_split_inside_st_is_delivered_once() {
        // TS-1: an alternate-screen OSC 11;? whose ST is split between
        // the ESC and the backslash, with only the ESC-ending half
        // suppressed. FR1: an ESC that is an OSC string's last available
        // byte leaves it INCOMPLETE, not aborted — the whole OSC (query
        // included) is held as a tail from its own start, ready for the
        // next (forwarded, not suppressed) chunk's lone backslash to
        // complete it exactly once. The backslash itself must never be
        // treated as separate ground text.
        let chunk = b"\x1b]11;?\x1b"; // alt-screen: nothing written to the ring
        let result = build(chunk, &[], &[]);
        assert_eq!(
            result, chunk,
            "the incomplete OSC (including its query) must be resent verbatim as a tail"
        );
    }

    /// Feed `bytes` to a fresh client wired with the real theme's OSC
    /// responder, drive it to completion, and return whatever response
    /// bytes it queued — the same "what would the live client answer"
    /// oracle AC-1 calls for, independent of this classifier's own logic.
    #[cfg(feature = "gui")]
    fn oracle_responses(bytes: &[u8]) -> Vec<u8> {
        use crate::callbacks::{NativeCallbackState, ThemeColorResponder};
        use crate::render::theme::Theme;
        use parking_lot::Mutex;
        use std::sync::Arc;
        use term_core::terminal_core::TerminalCore;

        let theme = Arc::new(Mutex::new(Theme::default()));
        let state = Arc::new(Mutex::new(NativeCallbackState::default()));
        let mut core = TerminalCore::new(80, 24, 1_000);
        core.osc_responder = Some(Box::new(ThemeColorResponder::new(theme, state)));
        core.process_pty_data_fully(bytes);
        core.take_response()
    }

    /// AC-1: each corpus case is checked by feeding a fresh client model
    /// this builder's replacement and comparing its responses against a
    /// reference client fed the raw stream directly (term_core, plus the
    /// theme for color responses). `ExactBytes` cases have no
    /// cursor-position-dependent response in play; `CountOnly` cases
    /// involve a CSI 6n preceded by state that shifts the cursor, where
    /// only response PRESENCE is required to match (FR9: the timing/
    /// content difference of the cursor-position response is accepted).
    #[cfg(feature = "gui")]
    #[test]
    fn replacement_matches_client_reference_for_transition_corpus() {
        enum OracleMode {
            ExactBytes,
            CountOnly,
        }

        let cases: &[(&str, &[u8], OracleMode)] = &[
            (
                "designator swallows ESC[6n",
                b"\x1b(\x1b[6n",
                OracleMode::ExactBytes,
            ),
            (
                "ESC X two-byte dispatch then a fresh CSI query",
                b"\x1bXhi\x1b[6n",
                OracleMode::CountOnly,
            ),
            (
                "DCS aborted by an embedded query",
                b"\x1bPq\x1b[6n\x1b\\",
                OracleMode::ExactBytes,
            ),
            (
                "ESC ESC stays in escape, CSI c dispatches",
                b"\x1b\x1b[c",
                OracleMode::ExactBytes,
            ),
            (
                "OSC number with a leading zero",
                b"\x1b]010;?\x07",
                OracleMode::ExactBytes,
            ),
            (
                "OSC 10 two chained query items",
                b"\x1b]10;?;?\x07",
                OracleMode::ExactBytes,
            ),
            (
                "OSC 10 set then query",
                b"\x1b]10;#fff;?\x07",
                OracleMode::ExactBytes,
            ),
            (
                "OSC 4 one query pair among a set pair",
                b"\x1b]4;1;?;2;#000000\x07",
                OracleMode::ExactBytes,
            ),
            (
                "aborted OSC 11 query",
                b"\x1b]11;?\x1bX",
                OracleMode::ExactBytes,
            ),
        ];

        for (name, raw, mode) in cases {
            let reference = oracle_responses(raw);
            let built = build_suppressed_replacement(raw, &[], &[], &[]);
            let client = oracle_responses(&built);
            match mode {
                OracleMode::ExactBytes => assert_eq!(
                    client, reference,
                    "case {name:?}: response bytes must match the reference exactly"
                ),
                OracleMode::CountOnly => {
                    assert_eq!(
                        client.is_empty(),
                        reference.is_empty(),
                        "case {name:?}: response presence must match the reference"
                    );
                    assert!(
                        !reference.is_empty(),
                        "case {name:?}: sanity check — the reference itself produced no response"
                    );
                }
            }
        }
    }

    // ---- round 2 (mux-suppressed-output-round2-fixes) ----

    const ESC: u8 = 0x1b;

    /// The responses a live client queues after being fed `bytes`: term_core
    /// with the theme's OSC responder installed when the `gui` feature
    /// provides one (color queries produce no response without it).
    fn client_responses(bytes: &[u8]) -> Vec<u8> {
        use term_core::terminal_core::TerminalCore;
        let mut core = TerminalCore::new(80, 24, 1_000);
        #[cfg(feature = "gui")]
        {
            use crate::callbacks::{NativeCallbackState, ThemeColorResponder};
            use crate::render::theme::Theme;
            use parking_lot::Mutex;
            use std::sync::Arc;

            let theme = Arc::new(Mutex::new(Theme::default()));
            let state = Arc::new(Mutex::new(NativeCallbackState::default()));
            core.osc_responder = Some(Box::new(ThemeColorResponder::new(theme, state)));
        }
        core.process_pty_data_fully(bytes);
        core.take_response()
    }

    /// A window of exactly [`client_parity_scan::RETAINED_WINDOW_BYTES`]:
    /// `prefix` then `fill` bytes.
    fn full_window(prefix: &[u8], fill: u8) -> Vec<u8> {
        assert!(prefix.len() <= client_parity_scan::RETAINED_WINDOW_BYTES);
        let mut window = prefix.to_vec();
        window.resize(client_parity_scan::RETAINED_WINDOW_BYTES, fill);
        assert_eq!(window.len(), client_parity_scan::RETAINED_WINDOW_BYTES);
        window
    }

    /// AC-2 (FR1, TS-1, TM-1; regression test for review round 2 finding
    /// `ecc48041b65a5380`): an ESC that can sit in a charset designator slot
    /// is never a restart position for a full retained window. Each case
    /// below is a stream on which the live client consumes that ESC as a
    /// designator byte, so it never starts the query-shaped bytes that
    /// follow; the replacement must be empty and a client fed it (plus a
    /// BEL) must queue no response.
    #[test]
    fn round2_ecc48041_designator_slot_esc_is_never_a_window_restart_position() {
        // (label, stream bytes BEFORE the retained window, window, chunk)
        type Case = (String, Vec<u8>, Vec<u8>, Vec<u8>);
        let mut cases: Vec<Case> = vec![
            (
                "ESC ( ESC ]10; + 249 spaces, chunk `?` BEL".to_string(),
                Vec::new(),
                full_window(&[ESC, b'(', ESC, b']', b'1', b'0', b';'], b' '),
                b"?\x07".to_vec(),
            ),
            (
                "ESC ( ESC [ + 252 NULs, chunk `c`".to_string(),
                Vec::new(),
                full_window(&[ESC, b'(', ESC, b'['], 0x00),
                b"c".to_vec(),
            ),
        ];
        // The chain form, with both parities. The pre-fix scan walked every
        // other ESC of a chain as a real start; which ESCs the live client
        // treats as designator bytes depends on the stream before the
        // window, so an even chain is preceded by `ESC (` (its first ESC is
        // then a designator byte) to keep the final ESC in a designator slot.
        for pairs in [1usize, 2, 3, 60, 61] {
            let before: Vec<u8> = if pairs % 2 == 0 {
                vec![ESC, b'(']
            } else {
                Vec::new()
            };
            let chain: Vec<u8> = [ESC, b'('].repeat(pairs);

            let mut osc_prefix = chain.clone();
            osc_prefix.extend_from_slice(&[ESC, b']', b'1', b'0', b';']);
            cases.push((
                format!("{pairs}x `ESC (` chain then ESC ]10;, chunk `?` BEL"),
                before.clone(),
                full_window(&osc_prefix, b' '),
                b"?\x07".to_vec(),
            ));

            let mut csi_prefix = chain;
            csi_prefix.extend_from_slice(&[ESC, b'[']);
            cases.push((
                format!("{pairs}x `ESC (` chain then ESC [ + NULs, chunk `c`"),
                before,
                full_window(&csi_prefix, 0x00),
                b"c".to_vec(),
            ));
        }

        for (label, before, window, chunk) in cases {
            // Premise: the live client, fed the raw stream, answers nothing.
            let mut raw = before;
            raw.extend_from_slice(&window);
            raw.extend_from_slice(&chunk);
            raw.push(0x07);
            assert!(
                client_responses(&raw).is_empty(),
                "case {label}: the raw stream must produce no response in the client"
            );

            let replacement = build_suppressed_replacement(&chunk, &[], &[], &window);
            assert!(
                replacement.is_empty(),
                "case {label}: a designator-slot ESC must never restart the scan, got {:?}",
                String::from_utf8_lossy(&replacement)
            );
            let mut delivered = replacement;
            delivered.push(0x07);
            assert!(
                client_responses(&delivered).is_empty(),
                "case {label}: the client must queue no response"
            );
        }
    }

    /// AC-3 (FR1 positive): a full window that holds a decidable ESC is
    /// scanned from there and the color query is delivered exactly once.
    #[test]
    fn full_window_with_a_decidable_esc_delivers_the_color_query_once() {
        let window = full_window(&[b'x', ESC, b']', b'1', b'0', b';'], b' ');
        let chunk = b"?\x07".to_vec();
        let result = build_suppressed_replacement(&chunk, &[], &[], &window);
        let mut expected = window[1..].to_vec();
        expected.extend_from_slice(&chunk);
        assert_eq!(result, expected);
    }

    /// AC-3 (FR1): a window shorter than the cap is scanned from its start.
    #[test]
    fn short_window_is_scanned_from_its_start_even_when_it_begins_with_esc() {
        let window = [ESC, b']', b'1', b'0', b';', b' ', b' '];
        let chunk = b"?\x07".to_vec();
        let result = build_suppressed_replacement(&chunk, &[], &[], &window);
        let mut expected = window.to_vec();
        expected.extend_from_slice(&chunk);
        assert_eq!(result, expected);
    }

    /// AC-4 (FR2, builder level): a color query completed in the chunk is
    /// included verbatim exactly once whether it lies inside a ring-written
    /// range, outside it, or across a range edge.
    #[test]
    fn color_query_is_included_once_whatever_its_ring_written_overlap() {
        // `A ESC ] 1 1 ; ? BEL B`: the query occupies bytes 1..8.
        let chunk = b"A\x1b]11;?\x07B";
        let query = b"\x1b]11;?\x07";
        let layouts: [(&str, Vec<Range<usize>>); 6] = [
            ("inside a range", vec![0..chunk.len()]),
            ("outside every range", vec![]),
            ("range ends inside the query", vec![0..4]),
            ("range starts inside the query", vec![5..chunk.len()]),
            ("straddled by two ranges", vec![0..4, 6..chunk.len()]),
            ("ranges on both sides only", vec![0..1, 8..9]),
        ];
        for (label, ranges) in layouts {
            let result = build(chunk, &ranges, &[]);
            assert_eq!(result, query, "layout {label}: exactly one verbatim copy");
        }
    }

    /// AC-4 (FR2): queries, color queries and viewer launches come out in
    /// stream order whatever the ring-written layout, each exactly once.
    #[test]
    fn color_queries_keep_stream_order_with_csi_queries_and_viewer_launches() {
        let items: [&[u8]; 5] = [
            b"\x1b[6n",
            b"\x1b]11;?\x07",
            b"\x1b]777;emterm;markdown;begin;id=1\x07",
            b"\x1b]10;?\x1b\\",
            b"\x1b[c",
        ];
        // Plain text between the items is never part of the replacement.
        let mut chunk = Vec::new();
        let mut expected = Vec::new();
        for item in items {
            chunk.extend_from_slice(b"text");
            chunk.extend_from_slice(item);
            expected.extend_from_slice(item);
        }
        // The whole chunk, none of it, and ranges cutting through items.
        let layouts: [Vec<Range<usize>>; 4] = [
            vec![0..chunk.len()],
            vec![],
            vec![2..14, 20..chunk.len()],
            vec![0..4, 8..13, 30..44],
        ];
        for ranges in layouts {
            let result = build(&chunk, &ranges, &[]);
            assert_eq!(result, expected, "ring-written ranges {ranges:?}");
        }
    }

    /// AC-4 (FR2): a color query that completed inside the retained window
    /// already reached the client in an earlier read and is not included.
    #[test]
    fn color_query_completed_inside_the_window_is_not_included() {
        let window = b"prefix\x1b]11;?\x07".to_vec();
        let chunk = b"plain text".to_vec();
        for ranges in [vec![0..chunk.len()], vec![]] {
            let result = build_suppressed_replacement(&chunk, &ranges, &[], &window);
            assert!(result.is_empty(), "ring-written ranges {ranges:?}");
        }
        // A query that STARTED in the window and completes in the chunk is.
        let window = b"prefix\x1b]11;".to_vec();
        let chunk = b"?\x07".to_vec();
        let result = build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], &window);
        assert_eq!(result, b"\x1b]11;?\x07");
    }

    // ---- FR8 (mux-suppressed-output-round2-fixes task0004): the snapshot
    // trailing construct ----

    /// Build with a snapshot trailing construct and a retained window.
    fn build_fr8(
        window: &[u8],
        chunk: &[u8],
        ring_written_ranges: &[Range<usize>],
        pending: &[u8],
        construct: Option<&[u8]>,
    ) -> Vec<u8> {
        build_suppressed_replacement_for(&SuppressedReplacementRequest {
            chunk,
            ring_written_ranges,
            pending_after: pending,
            window,
            snapshot_trailing_construct: construct,
            carried_over_completion: None,
        })
    }

    #[test]
    fn fr8_awaiting_designator_tail_equal_to_the_construct_with_no_items_is_omitted() {
        let chunk = b"abc\x1b(";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b("));
        assert!(
            result.is_empty(),
            "the client already holds `ESC (`: nothing to re-send, got {result:?}"
        );
        let chunk = b"abc\x1b)";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b)"));
        assert!(
            result.is_empty(),
            "the client already holds `ESC )`: nothing to re-send, got {result:?}"
        );
    }

    #[test]
    fn fr8_utf8_tail_equal_to_the_construct_with_no_items_is_omitted() {
        let mut chunk = b"abc".to_vec();
        chunk.extend_from_slice(&[0xe4, 0xb8]);
        let result = build_fr8(&[], &chunk, &[0..chunk.len()], &[], Some(&[0xe4, 0xb8]));
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn fr8_csi_tail_equal_to_the_construct_with_no_items_is_omitted() {
        let chunk = b"abc\x1b[3";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b[3"));
        assert!(result.is_empty(), "got {result:?}");
    }

    #[test]
    fn fr8_csi_tail_is_compared_after_c0_removal() {
        // The decider's construct has C0 removed; the builder's tail is
        // compared after its own C0 removal, so a CR inside the cut CSI
        // does not defeat the match.
        let chunk = b"abc\x1b[1\r;";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b[1;"));
        assert!(result.is_empty(), "got {result:?}");
        // The raw (C0 included) form is a different byte string: unchanged.
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b[1\r;"));
        assert_eq!(result, b"\x1b[1;");
    }

    #[test]
    fn fr8_tail_starting_in_the_window_is_compared_as_a_whole() {
        // The cut sequence began in the retained window and ends the chunk.
        let window = b"ab\x1b[";
        let chunk = b"3";
        let result = build_fr8(window, chunk, &[0..chunk.len()], &[], Some(b"\x1b[3"));
        assert!(result.is_empty(), "got {result:?}");
        // A construct that is only a prefix of the tail is not equal.
        let result = build_fr8(window, chunk, &[0..chunk.len()], &[], Some(b"\x1b["));
        assert_eq!(result, b"\x1b[3");
    }

    #[test]
    fn fr8_construct_absent_or_different_leaves_the_output_unchanged() {
        let chunk = b"abc\x1b(";
        let ranges = [0..chunk.len()];
        assert_eq!(build_fr8(&[], chunk, &ranges, &[], None), b"\x1b(");
        assert_eq!(
            build_fr8(&[], chunk, &ranges, &[], Some(b"\x1b)")),
            b"\x1b("
        );
        assert_eq!(
            build_fr8(&[], chunk, &ranges, &[], Some(b"\x1b[3")),
            b"\x1b("
        );
        // Wrapper form (no construct) is the same as an absent construct.
        assert_eq!(build(chunk, &ranges, &[]), b"\x1b(");
        let mut utf8 = b"abc".to_vec();
        utf8.extend_from_slice(&[0xe4, 0xb8]);
        assert_eq!(
            build_fr8(&[], &utf8, &[0..utf8.len()], &[], Some(&[0xe4])),
            vec![0xe4, 0xb8]
        );
    }

    #[test]
    fn fr8_designator_construct_with_a_csi_query_emits_filler_items_then_tail() {
        let chunk = b"\x1b[6nabc\x1b(";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b("));
        assert_eq!(
            result, b"B\x1b[6n\x1b(",
            "filler designator, the query, then the tail"
        );
        let chunk = b"\x1b[cabc\x1b)";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b)"));
        assert_eq!(result, b"B\x1b[c\x1b)");
    }

    #[test]
    fn fr8_designator_construct_with_an_alt_span_color_query_emits_filler_items_then_tail() {
        // The color query lies outside the ring-written ranges (it was in an
        // alternate-screen span), so it is re-delivered; `abc\x1b(` is the
        // ring-written part.
        let chunk = b"\x1b]11;?\x07abc\x1b(";
        let start = b"\x1b]11;?\x07".len();
        let result = build_fr8(&[], chunk, &[start..chunk.len()], &[], Some(b"\x1b("));
        assert_eq!(result, b"B\x1b]11;?\x07\x1b(");
    }

    #[test]
    fn fr8_designator_construct_with_a_viewer_launch_emits_filler_items_then_tail() {
        let chunk = b"\x1b]9999;emterm-md;# hi\x07abc\x1b(";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b("));
        assert_eq!(result, b"B\x1b]9999;emterm-md;# hi\x07\x1b(");
    }

    #[test]
    fn fr8_designator_construct_that_differs_from_the_tail_gets_no_filler() {
        let chunk = b"\x1b[6nabc\x1b(";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b)"));
        assert_eq!(result, b"\x1b[6n\x1b(");
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], None);
        assert_eq!(result, b"\x1b[6n\x1b(");
    }

    #[test]
    fn fr8_utf8_construct_with_items_emits_items_then_tail_with_no_filler() {
        let mut chunk = b"\x1b[6nabc".to_vec();
        chunk.extend_from_slice(&[0xe4, 0xb8]);
        let result = build_fr8(&[], &chunk, &[0..chunk.len()], &[], Some(&[0xe4, 0xb8]));
        let mut expected = b"\x1b[6n".to_vec();
        expected.extend_from_slice(&[0xe4, 0xb8]);
        assert_eq!(result, expected);
    }

    #[test]
    fn fr8_csi_construct_with_items_emits_items_then_tail_with_no_filler() {
        let chunk = b"\x1b[6nabc\x1b[3";
        let result = build_fr8(&[], chunk, &[0..chunk.len()], &[], Some(b"\x1b[3"));
        assert_eq!(result, b"\x1b[6n\x1b[3");
    }

    #[test]
    fn fr8_pending_run_tail_is_never_affected_by_the_construct() {
        // The tail comes from the write filter's pending run (D4 rule 1),
        // so even a construct byte-equal to it changes nothing.
        let pending = b"\x1b]9999;emterm-md;partial".to_vec();
        let mut chunk = b"abc".to_vec();
        chunk.extend_from_slice(&pending);
        let ranges = [0..chunk.len()];
        let with_construct = build_fr8(&[], &chunk, &ranges, &pending, Some(&pending));
        let without = build_fr8(&[], &chunk, &ranges, &pending, None);
        assert_eq!(with_construct, without);
        assert_eq!(with_construct, pending);
        // A pending run that equals an `ESC (` construct is likewise re-sent.
        let chunk = b"abc\x1b(";
        let ranges = [0..chunk.len()];
        let result = build_fr8(&[], chunk, &ranges, b"\x1b(", Some(b"\x1b("));
        assert_eq!(result, b"\x1b(");
    }

    // ---- mux-suppressed-output-round2-fixes FR5/FR6: excluded pieces and the
    // carried-over completion ----

    fn build_with_carried(
        chunk: &[u8],
        ring_written_ranges: &[Range<usize>],
        pending: &[u8],
        window: &[u8],
        carried: Option<(&[u8], usize)>,
    ) -> Vec<u8> {
        build_suppressed_replacement_for(&SuppressedReplacementRequest {
            chunk,
            ring_written_ranges,
            pending_after: pending,
            window,
            snapshot_trailing_construct: None,
            carried_over_completion: carried
                .map(|(bytes, end)| CarriedOverCompletion { bytes, end }),
        })
    }

    const SHORT_LAUNCH: &[u8] = b"\x1b]777;emterm;markdown;begin;id=short\x07";

    #[test]
    fn excluded_pieces_are_the_trailing_contribution_split_across_ranges() {
        let ranges = [0..5, 25..26];
        assert_eq!(excluded_pieces(&ranges, 0), Vec::<Range<usize>>::new());
        assert_eq!(excluded_pieces(&ranges, 1), vec![25..26]);
        assert_eq!(excluded_pieces(&ranges, 2), vec![4..5, 25..26]);
        assert_eq!(excluded_pieces(&ranges, 6), vec![0..5, 25..26]);
        assert_eq!(
            excluded_pieces(&ranges, 100),
            vec![0..5, 25..26],
            "a contribution beyond the total yields every range"
        );
    }

    #[test]
    fn excluded_pieces_skip_empty_ranges_and_never_cover_gaps() {
        let ranges = [0..3, 5..5, 7..9];
        assert_eq!(excluded_pieces(&ranges, 4), vec![1..3, 7..9]);
        assert!(excluded_pieces(&[], 3).is_empty());
    }

    #[test]
    fn pending_spanning_two_ranges_keeps_the_alt_range_query_and_appends_the_pending_tail() {
        // `ESC ] 2 ; x` (main) `ESC [?1049h` `ESC [6n` (alt) `ESC [?1049l` `y` (main)
        let mut chunk = b"\x1b]2;x".to_vec();
        chunk.extend_from_slice(b"\x1b[?1049h\x1b[6n\x1b[?1049l");
        chunk.push(b'y');
        let ranges = [0..5, 25..26];
        let pending = b"\x1b]2;xy";
        let out = build_with_carried(&chunk, &ranges, pending, &[], None);
        assert_eq!(out, [&b"\x1b[6n"[..], &pending[..]].concat());
    }

    #[test]
    fn carried_over_launch_whose_start_is_out_of_the_window_is_delivered() {
        let end = SHORT_LAUNCH.len() - 12;
        let mut chunk = SHORT_LAUNCH[12..].to_vec();
        chunk.extend_from_slice(b"post");
        let out = build_with_carried(
            &chunk,
            &[0..chunk.len()],
            &[],
            &[],
            Some((SHORT_LAUNCH, end)),
        );
        assert_eq!(out, SHORT_LAUNCH);
    }

    #[test]
    fn carried_over_launch_also_found_through_the_window_is_delivered_once() {
        let end = SHORT_LAUNCH.len() - 12;
        let mut chunk = SHORT_LAUNCH[12..].to_vec();
        chunk.extend_from_slice(b"post");
        let out = build_with_carried(
            &chunk,
            &[0..chunk.len()],
            &[],
            &SHORT_LAUNCH[..12],
            Some((SHORT_LAUNCH, end)),
        );
        assert_eq!(
            out, SHORT_LAUNCH,
            "same end position means the same sequence"
        );
    }

    #[test]
    fn a_launch_with_identical_content_ending_elsewhere_is_a_different_sequence() {
        let end = SHORT_LAUNCH.len() - 12;
        let mut chunk = SHORT_LAUNCH[12..].to_vec();
        chunk.extend_from_slice(SHORT_LAUNCH);
        let out = build_with_carried(
            &chunk,
            &[0..chunk.len()],
            &[],
            &SHORT_LAUNCH[..12],
            Some((SHORT_LAUNCH, end)),
        );
        assert_eq!(out, [SHORT_LAUNCH, SHORT_LAUNCH].concat());
    }

    #[test]
    fn carried_over_launch_precedes_a_later_alternate_range_query() {
        let end = SHORT_LAUNCH.len() - 12;
        let mut chunk = SHORT_LAUNCH[12..].to_vec();
        chunk.extend_from_slice(b"\x1b[?1049h\x1b[6n");
        let out = build_with_carried(&chunk, &[0..end], &[], &[], Some((SHORT_LAUNCH, end)));
        assert_eq!(out, [SHORT_LAUNCH, b"\x1b[6n"].concat());
    }

    #[test]
    fn carried_over_launch_and_the_pending_tail_are_both_delivered() {
        let end = SHORT_LAUNCH.len() - 12;
        let pending = b"\x1b]2;pa";
        let mut chunk = SHORT_LAUNCH[12..].to_vec();
        chunk.extend_from_slice(pending);
        let out = build_with_carried(
            &chunk,
            &[0..chunk.len()],
            pending,
            &[],
            Some((SHORT_LAUNCH, end)),
        );
        assert_eq!(out, [SHORT_LAUNCH, &pending[..]].concat());
    }

    #[test]
    fn carried_over_color_query_is_delivered_with_either_terminator() {
        for query in [&b"\x1b]11;?\x07"[..], &b"\x1b]11;?\x1b\\"[..]] {
            let split = 4;
            let chunk = query[split..].to_vec();
            let out = build_with_carried(
                &chunk,
                &[0..chunk.len()],
                &[],
                &[],
                Some((query, chunk.len())),
            );
            assert_eq!(out, query, "query {query:?}");
        }
    }

    #[test]
    fn carried_over_sequences_that_are_not_deliverable_are_dropped() {
        let cases: [(&str, &[u8]); 5] = [
            ("title OSC", b"\x1b]2;title\x07"),
            ("DCS", b"\x1bPqabc\x1b\\"),
            ("APC", b"\x1b_Gabc\x1b\\"),
            (
                "image launch",
                b"\x1b]777;emterm;image;chunk;id=1;data=AA\x07",
            ),
            ("not an OSC at all", b"\x1b[6n"),
        ];
        for (name, seq) in cases {
            let chunk = seq[3..].to_vec();
            let out = build_with_carried(
                &chunk,
                &[0..chunk.len()],
                &[],
                &[],
                Some((seq, chunk.len())),
            );
            assert!(out.is_empty(), "{name} must not be delivered: {out:?}");
        }
    }

    #[test]
    fn is_deliverable_osc_rejects_unterminated_and_degenerate_input() {
        assert!(is_deliverable_osc(SHORT_LAUNCH));
        assert!(!is_deliverable_osc(b""));
        assert!(!is_deliverable_osc(b"\x1b]"));
        assert!(!is_deliverable_osc(b"\x1b]11;?"));
        assert!(!is_deliverable_osc(b"\x1b]11;?\\"));
        assert!(!is_deliverable_osc(b"x]11;?\x07"));
    }
    // ---- mux-suppressed-output-round3-fixes task0004 (FR6): prepare and
    // finalize ----

    /// The monolithic builder as it stood before the prepare / finalize split,
    /// kept as the equivalence oracle: same scan, same assembly, same
    /// tail-omission rule, applied in one function.
    fn legacy_build(request: &SuppressedReplacementRequest<'_>) -> Vec<u8> {
        let SuppressedReplacementRequest {
            chunk,
            ring_written_ranges,
            pending_after,
            window,
            snapshot_trailing_construct,
            carried_over_completion,
        } = *request;
        if chunk.is_empty() {
            return Vec::new();
        }
        let chunk_ends_in_ring_written = ring_written_ranges
            .last()
            .is_some_and(|r| r.end == chunk.len());
        if !pending_after.is_empty() && chunk_ends_in_ring_written {
            let fed_len: usize = ring_written_ranges
                .iter()
                .map(|r| r.end.saturating_sub(r.start))
                .sum();
            let contribution = pending_after.len().min(fed_len);
            let excluded = excluded_pieces(ring_written_ranges, contribution);
            let outcome = client_parity_scan::scan(window, chunk, &excluded);
            let mut out = assemble_items(&outcome, carried_over_completion.as_ref());
            out.extend_from_slice(pending_after);
            out
        } else {
            let outcome = client_parity_scan::scan(window, chunk, &[]);
            let mut out = assemble_items(&outcome, carried_over_completion.as_ref());
            let tail: Vec<u8> = match outcome.tail.clone() {
                Some(tail_range) => {
                    let tail_bytes = &outcome.combined[tail_range];
                    if outcome.tail_strip_c0 {
                        strip_c0(tail_bytes).collect()
                    } else {
                        tail_bytes.to_vec()
                    }
                }
                None => Vec::new(),
            };
            let already_carried = !tail.is_empty()
                && snapshot_trailing_construct
                    .is_some_and(|construct| construct == tail.as_slice());
            if already_carried {
                return carried_tail_output(out, tail);
            }
            out.extend_from_slice(&tail);
            out
        }
    }

    struct CorpusCase {
        name: &'static str,
        window: Vec<u8>,
        chunk: Vec<u8>,
        ranges: Vec<Range<usize>>,
        pending: Vec<u8>,
        carried: Option<(&'static [u8], usize)>,
    }

    fn corpus() -> Vec<CorpusCase> {
        let whole = |name, chunk: &[u8]| CorpusCase {
            name,
            window: Vec::new(),
            chunk: chunk.to_vec(),
            ranges: vec![0..chunk.len()],
            pending: Vec::new(),
            carried: None,
        };
        let utf8_tail = [b"abc".as_slice(), &[0xe4, 0xb8]].concat();
        let utf8_items = [b"\x1b[6nabc".as_slice(), &[0xe4, 0xb8]].concat();
        let pending_run = b"\x1b]9999;emterm-md;partial".to_vec();
        let with_pending = [b"abc".as_slice(), &pending_run].concat();
        let alt_query = b"\x1b]11;?\x07abc\x1b(".to_vec();
        let alt_start = b"\x1b]11;?\x07".len();
        let launch_chunk = [&SHORT_LAUNCH[12..], b"post"].concat();
        vec![
            whole("empty chunk", b""),
            whole("plain output", b"hello, world\r\n"),
            whole("awaiting designator tail", b"abc\x1b("),
            whole("awaiting designator tail, close form", b"abc\x1b)"),
            whole("cut utf-8 tail", &utf8_tail),
            whole("cut csi tail", b"abc\x1b[3"),
            whole("cut csi tail with an inner c0", b"abc\x1b[1\r;"),
            whole("csi query then designator tail", b"\x1b[6nabc\x1b("),
            whole("csi query then csi tail", b"\x1b[6nabc\x1b[3"),
            whole("csi query then utf-8 tail", &utf8_items),
            whole(
                "viewer launch then designator tail",
                b"\x1b]9999;emterm-md;# hi\x07abc\x1b(",
            ),
            whole("color query only", b"\x1b]11;?\x07"),
            whole("incomplete osc query as the tail", b"\x1b]11;?\x1b"),
            CorpusCase {
                name: "color query in an alternate span, designator tail",
                window: Vec::new(),
                ranges: vec![alt_start..alt_query.len()],
                chunk: alt_query,
                pending: Vec::new(),
                carried: None,
            },
            CorpusCase {
                name: "pending run is the tail",
                window: Vec::new(),
                ranges: vec![0..with_pending.len()],
                chunk: with_pending,
                pending: pending_run,
                carried: None,
            },
            CorpusCase {
                name: "pending run equal to a designator construct",
                window: Vec::new(),
                chunk: b"abc\x1b(".to_vec(),
                ranges: vec![0..5],
                pending: b"\x1b(".to_vec(),
                carried: None,
            },
            CorpusCase {
                name: "tail starting in the window",
                window: b"ab\x1b[".to_vec(),
                chunk: b"3".to_vec(),
                ranges: vec![0..1],
                pending: Vec::new(),
                carried: None,
            },
            CorpusCase {
                name: "carried-over launch with a designator tail",
                window: Vec::new(),
                ranges: vec![0..launch_chunk.len()],
                chunk: launch_chunk,
                pending: Vec::new(),
                carried: Some((SHORT_LAUNCH, SHORT_LAUNCH.len() - 12)),
            },
        ]
    }

    fn corpus_constructs() -> Vec<Option<Vec<u8>>> {
        vec![
            None,
            Some(b"\x1b(".to_vec()),
            Some(b"\x1b)".to_vec()),
            Some(b"\x1b[3".to_vec()),
            Some(b"\x1b[1;".to_vec()),
            Some(b"\x1b[1\r;".to_vec()),
            Some(vec![0xe4, 0xb8]),
            Some(vec![0xe4]),
            Some(b"\x1b]11;?\x1b".to_vec()),
            Some(b"\x1b]9999;emterm-md;partial".to_vec()),
        ]
    }

    /// AC-3 (FR6, TM-2): finalize applied to prepare equals the monolithic
    /// builder's output for a corpus of requests with and without a
    /// construct, and equals the public builders'.
    #[test]
    fn round3_finalize_of_prepare_equals_the_monolithic_builder_over_a_corpus() {
        let mut omissions = 0usize;
        let mut cases = 0usize;
        for case in corpus() {
            for construct in corpus_constructs() {
                let carried = case
                    .carried
                    .map(|(bytes, end)| CarriedOverCompletion { bytes, end });
                let request = SuppressedReplacementRequest {
                    chunk: &case.chunk,
                    ring_written_ranges: &case.ranges,
                    pending_after: &case.pending,
                    window: &case.window,
                    snapshot_trailing_construct: construct.as_deref(),
                    carried_over_completion: carried,
                };
                let expected = legacy_build(&request);
                let composed = finalize_suppressed_replacement(
                    prepare_suppressed_replacement(&request),
                    construct.as_deref(),
                );
                assert_eq!(
                    composed, expected,
                    "case {:?}, construct {construct:?}",
                    case.name
                );
                assert_eq!(
                    build_suppressed_replacement_for(&request),
                    expected,
                    "case {:?}, construct {construct:?}: the builder keeps its result",
                    case.name
                );
                let without = SuppressedReplacementRequest {
                    snapshot_trailing_construct: None,
                    ..request
                };
                if construct.is_some() && legacy_build(&without) != expected {
                    omissions += 1;
                }
                cases += 1;
            }
        }
        assert!(cases >= 100, "the corpus must be broad, ran {cases}");
        assert!(
            omissions >= 5,
            "the corpus must exercise the tail-omission rule, only {omissions} cases changed"
        );
    }

    /// AC-3: one prepared value finalizes correctly against any construct —
    /// the decision is the only thing finalize looks at, and prepare does not
    /// read the request's own construct.
    #[test]
    fn round3_one_prepared_value_finalizes_against_each_construct() {
        let chunk = b"\x1b[6nabc\x1b(";
        let ranges = [0..chunk.len()];
        let request = |construct| SuppressedReplacementRequest {
            chunk,
            ring_written_ranges: &ranges,
            pending_after: &[],
            window: &[],
            snapshot_trailing_construct: construct,
            carried_over_completion: None,
        };
        // Prepared with a matching construct in the request: still the same
        // prepared value as without one.
        let prepared = prepare_suppressed_replacement(&request(Some(b"\x1b(")));
        assert!(!prepared.is_empty());
        let again = prepare_suppressed_replacement(&request(None));
        assert_eq!(prepared.items, again.items);
        assert_eq!(prepared.tail, again.tail);
        assert_eq!(prepared.tail_omittable, again.tail_omittable);

        assert_eq!(
            finalize_suppressed_replacement(again, None),
            b"\x1b[6n\x1b(",
            "no construct: the tail is sent after the items"
        );
        assert_eq!(
            finalize_suppressed_replacement(prepared, Some(b"\x1b(")),
            b"B\x1b[6n\x1b(",
            "a matching construct: filler, items, tail"
        );
    }

    /// AC-3: the write filter's pending-run tail is never omittable, and a
    /// prepared value with neither items nor a tail is empty whatever the
    /// construct.
    #[test]
    fn round3_pending_tail_is_not_omittable_and_an_empty_prepare_stays_empty() {
        let pending = b"\x1b(";
        let chunk = b"abc\x1b(";
        let prepared = prepare_suppressed_replacement(&SuppressedReplacementRequest {
            chunk,
            ring_written_ranges: &[0..chunk.len()],
            pending_after: pending,
            window: &[],
            snapshot_trailing_construct: None,
            carried_over_completion: None,
        });
        assert!(!prepared.tail_omittable);
        assert_eq!(
            finalize_suppressed_replacement(prepared, Some(b"\x1b(")),
            b"\x1b("
        );

        let plain = b"hello\r\n";
        let prepared = prepare_suppressed_replacement(&SuppressedReplacementRequest {
            chunk: plain,
            ring_written_ranges: &[0..plain.len()],
            pending_after: &[],
            window: &[],
            snapshot_trailing_construct: None,
            carried_over_completion: None,
        });
        assert!(prepared.is_empty());
        for construct in corpus_constructs() {
            let again = prepare_suppressed_replacement(&SuppressedReplacementRequest {
                chunk: plain,
                ring_written_ranges: &[0..plain.len()],
                pending_after: &[],
                window: &[],
                snapshot_trailing_construct: None,
                carried_over_completion: None,
            });
            assert!(
                finalize_suppressed_replacement(again, construct.as_deref()).is_empty(),
                "construct {construct:?}"
            );
        }
    }
}
