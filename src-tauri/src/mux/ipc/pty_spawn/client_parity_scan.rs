//! Client-parity byte classifier (mux-suppressed-output-fixes task0001).
//!
//! Walks bytes the same way the client's `term_core` parser does (SPEC.md
//! "走査の遷移規則（FR2）"), at sequence granularity, so the suppressed-chunk
//! replacement builder ([`super::suppressed_output`]) extracts exactly the
//! queries and viewer launches the client's live parser would have acted on
//! — no more (TM-1: never fabricate from a position the client's parser
//! would never start a sequence at) and, within the retained-window bound,
//! no fewer.
//!
//! # Combined-buffer model
//!
//! [`scan`] takes the reader's retained window (the stream bytes preceding
//! the suppressed chunk, up to [`RETAINED_WINDOW_BYTES`]) and the chunk
//! itself, and scans them as ONE continuous buffer (`combined = window[s..]
//! ++ chunk`), where `s` is the FR6 start-state derivation offset computed
//! by [`derive_prefix_start`]. Working over one combined buffer — rather
//! than tracking a separate "start state" enum — means a sequence that
//! began in the window and completes in the chunk is scanned exactly like
//! any other sequence: no special-casing at the window/chunk boundary.
//!
//! Items are reported only when their completing byte lies in the chunk
//! portion (`range.end > boundary`); an item whose start AND end both lie
//! in the window already reached the client in an earlier read and is
//! correctly excluded — see [`ScanOutcome`].
//!
//! # Why any single ESC (except the designator slot) resyncs safely
//!
//! `derive_prefix_start`'s window-prefix search relies on a structural
//! property of the client parser: seeing ESC ALWAYS forces a transition
//! toward the escape track, regardless of the parser's prior state —
//! whether that was Ground, CSI, or inside an OSC/DCS/APC string (FR2
//! (a)/(b)/(f) each abort their respective construct on ESC and hand the
//! interrupting byte to the same escape-dispatch the Ground path uses). The
//! ONE exception is the designator slot right after `ESC (` / `ESC )`: that
//! byte is consumed as data (the designator) even when its value is 0x1B
//! (FR2 (d)). So scanning forward from Ground at ANY ESC position in the
//! window reproduces the true parser's state at the window's end — as long
//! as that ESC is not itself sitting in a designator slot the window cannot
//! rule out, which is exactly what [`esc_may_be_designator_byte`] excludes.

use std::ops::Range;

use crate::mux::osc_identify::{OscIdentity, RecoveredOsc, identify_osc, recover_osc};
use crate::mux::scrollback_filter::scan_csi_device_query;

/// FR6/D2: the number of bytes of PTY stream the reader retains from before
/// the current read (a sliding window over previous reads, reader-thread-
/// local, no lock). Chosen to cover the unfinished part of any CSI device
/// query, an ESC plus designator (2 bytes), a UTF-8 lead plus continuations
/// (up to 4 bytes), long SGR parameter lists, and ordinary color-query OSCs.
pub(in crate::mux) const RETAINED_WINDOW_BYTES: usize = 256;

/// Kind of a reportable item extracted from a suppressed chunk (SPEC.md
/// "置換出力の構成と配送順序").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::mux) enum ScanItemKind {
    /// A CSI the client would answer (device query) — reused unchanged from
    /// [`scan_csi_device_query`], the existing SSOT.
    CsiQuery,
    /// An OSC the theme would answer (FR2 (g)/(h)).
    ColorQuery,
    /// A complete OSC 777/9999 viewer-launch sequence (FR7, D3).
    ViewerLaunch,
}

/// One extracted item: its kind and its byte range in [`ScanOutcome::combined`]'s
/// coordinate space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::mux) struct ScanItem {
    pub(in crate::mux) kind: ScanItemKind,
    pub(in crate::mux) range: Range<usize>,
}

/// Result of [`scan`]: the combined buffer that was walked, where the
/// suppressed chunk begins within it, every qualifying item (in stream
/// order), and the trailing incomplete construct, if scanning reached the
/// end of the chunk without resolving one (and no piece was excluded).
pub(in crate::mux) struct ScanOutcome {
    /// `window[s..] ++ chunk` (see the module doc).
    pub(in crate::mux) combined: Vec<u8>,
    /// `combined[boundary..] == chunk` exactly.
    pub(in crate::mux) boundary: usize,
    /// Items whose completing byte lies at or after `boundary` (i.e. inside
    /// `chunk`), in stream order. An item entirely inside the window
    /// (`range.end <= boundary`) already reached the client in an earlier
    /// read and is never reported here.
    pub(in crate::mux) items: Vec<ScanItem>,
    /// The trailing incomplete construct at the end of the chunk, if any
    /// (always `None` when [`scan`] was given excluded pieces). May
    /// start before `boundary` (a sequence that began in the window and is
    /// STILL incomplete at the chunk's end, FR6).
    pub(in crate::mux) tail: Option<Range<usize>>,
    /// Whether `tail`'s bytes need their C0 control bytes removed before
    /// re-delivery (FR3: true only for an in-progress CSI tail; C0 executed
    /// mid-CSI already took effect via the snapshot).
    pub(in crate::mux) tail_strip_c0: bool,
}

/// Scan a suppressed chunk for FR6/FR7/FR9 items and an FR1/FR3/FR6 tail.
///
/// `window` is the retained stream bytes preceding `chunk` (reader-local,
/// up to [`RETAINED_WINDOW_BYTES`]). `excluded` is a set of chunk-coordinate
/// pieces (ascending, non-overlapping) the write filter's pending run
/// occupies (mux-suppressed-output-round2-fixes FR5): the scan walks the
/// WHOLE chunk — so items in the gaps between the pieces (alternate-screen
/// ranges, removed switch sequences) are still found — but never reports an
/// item that overlaps a piece, and reports no tail when any piece is
/// excluded, because the pending run itself is the tail then (the bytes it
/// will re-deliver must never also be found here). An empty `excluded`
/// disables neither: items and the tail are discovered over the whole chunk.
///
/// Contract (NFR5/TM-2): a bounded forward pass per reading, work
/// proportional to `window.len() + chunk.len()` (plus one pass over
/// `excluded` per reported item); never panics.
///
/// Round-4 FR2 (as-05 fallback): when [`first_chunk_byte_may_be_designator`]
/// holds, the walk starts at the chunk and the chunk's first byte may or may
/// not have been consumed by the client as a designator, which the window
/// cannot decide. The scan then reads the same view two ways — reading (b)
/// parses the first byte from ground, reading (a) takes it as the consumed
/// designator and starts the same ground-state walk one byte later — and
/// keeps only what both readings report identically:
///
/// - an item, when both report an item with the same range and the same
///   kind;
/// - the tail, when both report the same tail start (its C0-strip flag is the
///   one that tail carries);
/// - an end-of-view check that does not depend on where the walk started
///   (the incomplete UTF-8 character at the end of the view) is evaluated
///   once on the shared view, so both readings report it identically.
///
/// Everything else is dropped. The intersection can only remove results, so
/// it can cause misses but never a fabricated item or tail; a designator
/// chain (`ESC ( ESC ( …`) inside the chunk can no longer turn an ESC that
/// the client consumed as a designator into the start of a sequence. Outside
/// the fallback condition there is exactly one reading, from the chunk's
/// start, as before.
pub(in crate::mux) fn scan(window: &[u8], chunk: &[u8], excluded: &[Range<usize>]) -> ScanOutcome {
    scan_with_reading_count(window, chunk, excluded).0
}

/// [`scan`], and the number of readings it ran: 2 under the as-05 fallback
/// condition ([`first_chunk_byte_may_be_designator`]), otherwise 1.
pub(in crate::mux::ipc::pty_spawn) fn scan_with_reading_count(
    window: &[u8],
    chunk: &[u8],
    excluded: &[Range<usize>],
) -> (ScanOutcome, usize) {
    let s = derive_prefix_start(window);
    // Round-4 FR2 (TM-1): in the as-05 fallback behind a trailing `ESC (` /
    // `ESC )` the chunk's first byte may have been consumed by the client as
    // the designator. The walk then starts at the chunk (`s` is the window's
    // end), so the window contributes nothing to the walked view.
    let first_byte_undecided = first_chunk_byte_may_be_designator(window, s);
    let mut combined = Vec::with_capacity((window.len() - s) + chunk.len());
    combined.extend_from_slice(&window[s..]);
    let boundary = combined.len();
    combined.extend_from_slice(chunk);

    let detect_tail = excluded.is_empty();
    // End-of-view check, independent of where a walk started: evaluated once
    // on the shared view.
    let utf8_tail = if detect_tail {
        utf8_tail_start_at_end(&combined)
    } else {
        None
    };

    // Reading (b): the walk from ground at the view's first byte.
    let from_ground = walk_view(&combined, 0, detect_tail);
    let (raw_items, tail, readings) = if first_byte_undecided {
        // Reading (a): the view's first byte is the consumed designator, so
        // the same ground-state walk starts one byte later (same
        // coordinates).
        let after_designator = walk_view(&combined, 1, detect_tail);
        let tail = same_tail(
            reading_tail(&from_ground, utf8_tail),
            reading_tail(&after_designator, utf8_tail),
        );
        let items = common_items(from_ground.items, &after_designator.items);
        (items, tail, 2)
    } else {
        let tail = reading_tail(&from_ground, utf8_tail);
        (from_ground.items, tail, 1)
    };

    let items = raw_items
        .into_iter()
        .filter(|item| {
            item.range.end > boundary
                && !overlaps_excluded(
                    item.range.start.saturating_sub(boundary),
                    item.range.end - boundary,
                    excluded,
                )
        })
        .collect();
    let (tail, tail_strip_c0) = match tail {
        Some((start, strip_c0)) => (Some(start..combined.len()), strip_c0),
        None => (None, false),
    };

    (
        ScanOutcome {
            combined,
            boundary,
            items,
            tail,
            tail_strip_c0,
        },
        readings,
    )
}

/// What one reading of the view reports: its items, and the trailing
/// incomplete construct its walk ended in (when tail discovery is on).
struct Reading {
    items: Vec<ScanItem>,
    /// Start of the construct the walk was still inside at the view's end.
    walk_tail_start: Option<usize>,
    /// Whether that construct is an in-progress CSI (FR3: its C0 bytes are
    /// removed before re-delivery).
    walk_tail_strip_c0: bool,
}

/// The tail a reading reports as `(start, strip C0)`: the construct its walk
/// ended in, otherwise the shared view's incomplete UTF-8 character.
fn reading_tail(reading: &Reading, utf8_tail: Option<usize>) -> Option<(usize, bool)> {
    match reading.walk_tail_start {
        Some(start) => Some((start, reading.walk_tail_strip_c0)),
        None => utf8_tail.map(|start| (start, false)),
    }
}

/// The tail both readings report: kept only when their starts are equal. The
/// C0-strip flag is the one that tail carries — it follows from the byte at
/// the start (a CSI strips, an incomplete OSC/DCS/APC, designator, bare ESC
/// or UTF-8 character does not), so equal starts carry equal flags.
fn same_tail(first: Option<(usize, bool)>, second: Option<(usize, bool)>) -> Option<(usize, bool)> {
    match (first, second) {
        (Some(a), Some(b)) if a.0 == b.0 => Some(a),
        _ => None,
    }
}

/// The items both readings report with the same range and the same kind, in
/// stream order. Each list is in ascending, non-overlapping order, so one
/// forward pass over both finds the common items.
fn common_items(first: Vec<ScanItem>, second: &[ScanItem]) -> Vec<ScanItem> {
    let mut common = Vec::new();
    let mut other = second.iter().peekable();
    for item in first {
        while other
            .next_if(|c| c.range.start < item.range.start)
            .is_some()
        {}
        if other.peek().is_some_and(|c| **c == item) {
            common.push(item);
        }
    }
    common
}

/// Whether the chunk-coordinate span `[start, end)` overlaps any excluded
/// piece.
fn overlaps_excluded(start: usize, end: usize, excluded: &[Range<usize>]) -> bool {
    excluded.iter().any(|p| start < p.end && end > p.start)
}

/// FR6 "Full window" / D2 "stream shorter than N": pick the offset within
/// `window` from which a plain Ground-state walk (through `window[s..]`
/// then the chunk) reproduces the true parser's state — see the module doc
/// for why any ESC except the designator slot is a safe resync point.
///
/// Round-2 FR1: the first ESC that [`esc_may_be_designator_byte`] does not
/// exclude. A single forward pass over the window with no panicking
/// indexing (TM-2).
fn derive_prefix_start(window: &[u8]) -> usize {
    if window.len() < RETAINED_WINDOW_BYTES {
        // D2: the window holds the entire stream so far — no truncation,
        // no ambiguity. Walk it from the very start.
        return 0;
    }
    let mut i = 0;
    while i < window.len() {
        if window[i] == 0x1b && !esc_may_be_designator_byte(window, i) {
            return i;
        }
        i += 1;
    }
    // as-05 fallback: no decidable ESC in the window. The state is Ground,
    // unless the window's own trailing bytes are an in-progress UTF-8
    // character — in which case start the walk there so the combined scan
    // naturally re-derives the UTF-8-partial state at the boundary.
    utf8_tail_start_at_end(window).unwrap_or(window.len())
}

/// Round-3 FR5 (TM-1, review finding eaf83fe08869d5e6): whether, in the
/// as-05 fallback, the chunk's first byte may already have been consumed by
/// the client's parser as a charset designator. Round-4 FR2 (review finding
/// 989ec5c588abce06): when it holds, [`scan`] reads the chunk two ways — the
/// first byte as the consumed designator, and from ground — and keeps only
/// what both readings report, because the undecidable designator parity
/// carries through the chunk (every `ESC (` chain inside it shifts which
/// ESC the client consumed).
///
/// The fallback applies when the window is at the retention size and holds
/// no ESC that [`esc_may_be_designator_byte`] accepts as a restart position
/// (`start` is then the window's end, or its trailing UTF-8 partial's lead
/// byte). If the window's last byte is `(` / `)` and that byte is either the
/// window's first byte or preceded by ESC, it may be a real `ESC (` / `ESC )`
/// introducer, in which case the chunk's first byte is its designator. Whether
/// it is depends on the parity of a designator chain that began before the
/// window, which the window cannot decide.
///
/// A trailing `(` / `)` preceded by anything other than ESC is a designator
/// byte or text, never an introducer, so it does not qualify. A window below
/// the retention size, a window with a decidable ESC, and a trailing UTF-8
/// partial (whose last byte is never `(` / `)`) never qualify either.
///
/// Reads at most the window's last two bytes (NFR5): no extra pass.
fn first_chunk_byte_may_be_designator(window: &[u8], start: usize) -> bool {
    if window.len() < RETAINED_WINDOW_BYTES || start != window.len() {
        return false;
    }
    if !matches!(window.last(), Some(b'(' | b')')) {
        return false;
    }
    // At the retention size the introducer is never the window's first byte
    // (`None`); the arm keeps the rule reading like the designator-slot
    // exclusion in [`esc_may_be_designator_byte`].
    match window.len().checked_sub(2) {
        None => true,
        Some(before_intro) => window.get(before_intro) == Some(&0x1b),
    }
}

/// Round-2 FR1 (TM-1): whether the ESC at `window[esc_pos]` may be the
/// designator byte the client's parser consumes right after `ESC (` /
/// `ESC )` (FR2 (d) — consumed as data even when its value is ESC), judged
/// from the window alone. Such an ESC never starts a sequence, so a walk
/// restarted there would fabricate queries from the bytes that follow it.
///
/// The ESC is excluded when it is the window's first byte (an `ESC (`
/// before the window is undecidable), or when the byte before it is `(` /
/// `)` and either that byte is the window's first byte or the byte before
/// that is ESC. The rule is applied at every candidate, so an
/// `ESC ( ESC ( …` chain excludes each of its ESCs in turn. It may exclude
/// an ESC the client would in fact have started a sequence at (an ESC
/// whose preceding ESC was itself a designator byte); that only loses a
/// query or launch, never fabricates one.
fn esc_may_be_designator_byte(window: &[u8], esc_pos: usize) -> bool {
    let Some(intro_pos) = esc_pos.checked_sub(1) else {
        return true;
    };
    if !matches!(window.get(intro_pos), Some(b'(' | b')')) {
        return false;
    }
    match intro_pos.checked_sub(1) {
        None => true,
        Some(before_intro) => window.get(before_intro) == Some(&0x1b),
    }
}

/// Outcome of scanning for a string's terminator (OSC/DCS/APC). Mirrors
/// FR2 (a)/(b): an ESC that is the string's last available byte leaves it
/// **incomplete** (FR1), never aborted.
enum StringScanResult {
    /// `(index of the terminator's first byte, index just past the
    /// terminator)`.
    Complete(usize, usize),
    /// A bare ESC not followed by `\` cancels the string here (FR2 (a)/(b));
    /// the string never completed, so nothing inside it was ever a
    /// protected payload (TM-1) — scanning resumes here as a fresh
    /// top-level position.
    Aborted(usize),
    /// The view ran out before a terminator appeared — including an ESC
    /// that was the view's very last byte (FR1: incomplete, not aborted).
    Unterminated,
}

/// Find an OSC's terminator (BEL or ST) in `view` starting at `from`.
fn find_osc_terminator(view: &[u8], from: usize) -> StringScanResult {
    let mut j = from;
    while j < view.len() {
        match view[j] {
            0x07 => return StringScanResult::Complete(j, j + 1),
            0x1b => {
                return match view.get(j + 1) {
                    None => StringScanResult::Unterminated,
                    Some(b'\\') => StringScanResult::Complete(j, j + 2),
                    Some(_) => StringScanResult::Aborted(j),
                };
            }
            _ => {}
        }
        j += 1;
    }
    StringScanResult::Unterminated
}

/// Find a DCS/APC string's ST terminator in `view` starting at `from`
/// (unlike OSC, these do not accept a bare BEL terminator).
fn find_st_terminator(view: &[u8], from: usize) -> StringScanResult {
    let mut j = from;
    while j < view.len() {
        if view[j] == 0x1b {
            return match view.get(j + 1) {
                None => StringScanResult::Unterminated,
                Some(b'\\') => StringScanResult::Complete(j, j + 2),
                Some(_) => StringScanResult::Aborted(j),
            };
        }
        j += 1;
    }
    StringScanResult::Unterminated
}

/// Whether the CSI candidate starting at `esc_pos` (`view[esc_pos] == ESC`,
/// `view[esc_pos + 1] == '['`) is incomplete because `view` ends before a
/// final byte (0x40-0x7E) or an aborting bare ESC is seen — genuinely cut
/// off at the view's end, as opposed to [`scan_csi_device_query`] returning
/// `None` for a CSI that completed within the view but simply is not a
/// device query (or was cancelled/aborted at an earlier position).
fn is_unterminated_csi_tail(view: &[u8], esc_pos: usize) -> bool {
    let mut j = esc_pos + 2;
    while j < view.len() {
        let b = view[j];
        if b == 0x1b || (0x40..=0x7e).contains(&b) {
            return false;
        }
        j += 1;
    }
    true
}

/// Whether `view` ends with an incomplete multi-byte UTF-8 sequence — a
/// lead byte within the last 4 bytes that expects more continuation bytes
/// than remain, with every byte after it a valid continuation byte
/// (0x80-0xBF). Returns the lead byte's position if so. Bounded (at most 4
/// bytes examined), run once per scan rather than at every position.
fn utf8_tail_start_at_end(view: &[u8]) -> Option<usize> {
    let len = view.len();
    let probe = 4.min(len);
    for back in 1..=probe {
        let start = len - back;
        let b = view[start];
        let expected_len = match b {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => continue,
        };
        if expected_len > back
            && view[start + 1..]
                .iter()
                .all(|&c| (0x80..=0xbf).contains(&c))
        {
            return Some(start);
        }
    }
    None
}

/// Single forward pass over `view` from `start` (FR2), classifying every
/// completed OSC/CSI as a reportable item when it qualifies (FR7/FR9), and
/// reporting the trailing incomplete construct when the walk reaches the
/// buffer's end without resolving one AND `detect_tail` is set (tail discovery
/// is disabled when the write filter's pending run owns the tail — FR5's
/// pending exclusion).
///
/// The walk begins in the ground state at `start` — 0 for the ordinary walk,
/// 1 for the as-05 reading that takes the view's first byte as a consumed
/// designator — and reports in the view's coordinates. The incomplete UTF-8
/// character at the view's end is not part of the walk (see [`scan`]).
fn walk_view(view: &[u8], start: usize, detect_tail: bool) -> Reading {
    let limit = view.len();
    let mut items = Vec::new();
    let mut pos = start;
    let mut tail_start: Option<usize> = None;
    let mut tail_strip_c0 = false;

    while pos < limit {
        if view[pos] != 0x1b {
            pos += 1;
            continue;
        }
        match view.get(pos + 1) {
            None => {
                // Bare ESC as the last byte we're allowed to look at (FR1:
                // incomplete, not aborted).
                if detect_tail {
                    tail_start = Some(pos);
                }
                break;
            }
            Some(b'[') => match scan_csi_device_query(view, pos + 2) {
                Some(strip) => {
                    items.push(ScanItem {
                        kind: ScanItemKind::CsiQuery,
                        range: pos..strip.end,
                    });
                    pos = strip.end;
                }
                None => {
                    if detect_tail && is_unterminated_csi_tail(view, pos) {
                        tail_start = Some(pos);
                        tail_strip_c0 = true;
                        break;
                    }
                    pos += 1;
                }
            },
            Some(b']') => match find_osc_terminator(view, pos + 2) {
                StringScanResult::Complete(body_end, term_end) => {
                    let osc = reconstruct_osc_number_and_data(&view[pos + 2..body_end]);
                    if is_color_query(&osc) {
                        items.push(ScanItem {
                            kind: ScanItemKind::ColorQuery,
                            range: pos..term_end,
                        });
                    } else if is_viewer_launch(&osc) {
                        items.push(ScanItem {
                            kind: ScanItemKind::ViewerLaunch,
                            range: pos..term_end,
                        });
                    }
                    pos = term_end;
                }
                StringScanResult::Aborted(abort_pos) => {
                    pos = abort_pos;
                }
                StringScanResult::Unterminated => {
                    if detect_tail {
                        tail_start = Some(pos);
                    }
                    break;
                }
            },
            Some(b'_') | Some(b'P') => match find_st_terminator(view, pos + 2) {
                StringScanResult::Complete(_, term_end) => {
                    pos = term_end;
                }
                StringScanResult::Aborted(abort_pos) => {
                    pos = abort_pos;
                }
                StringScanResult::Unterminated => {
                    if detect_tail {
                        tail_start = Some(pos);
                    }
                    break;
                }
            },
            Some(b'(') | Some(b')') => match view.get(pos + 2) {
                None => {
                    // FR2 (d): the designator byte has not arrived yet —
                    // incomplete, awaiting one more byte (consumed
                    // unconditionally once it does, even if it's ESC).
                    if detect_tail {
                        tail_start = Some(pos);
                    }
                    break;
                }
                Some(_) => {
                    pos += 3;
                }
            },
            Some(0x1b) => {
                // FR2 (e): ESC ESC stays in Escape — advancing by exactly 1
                // lands on the second ESC, which the next iteration
                // re-evaluates as the real decision point.
                pos += 1;
            }
            Some(_) => {
                // Every other byte completes a 2-byte escape dispatch
                // (SOS/PM, the known single-letter finals, and any other
                // byte all fall through term_core's `escape()` catch-all
                // identically) and returns to ground. Never an item.
                pos += 2;
            }
        }
    }

    Reading {
        items,
        walk_tail_start: tail_start,
        walk_tail_strip_c0: tail_strip_c0,
    }
}

/// FR2 (g): the OSC number and data exactly as term_core's OSC string state
/// would reconstruct them. Digits before the first `;` accumulate into the
/// number (leading zeros do not affect the value); the first `;` is
/// dropped; every other byte — including a digit run after the first `;`,
/// and any non-digit interleaved before it — is data. The data is decoded
/// lossily, matching `dispatch_osc`.
///
/// `number` is `None` when the accumulation would overflow term_core's
/// `u16` (as-06) — term_core's own arithmetic can wrap or panic there, so no
/// route can be reliably established; this reconstruction never panics
/// regardless (all arithmetic is saturating/checked).
///
/// The reconstruction itself lives in the shared OSC identification layer
/// ([`crate::mux::osc_identify`]) that the scrollback strip also uses
/// (mux-suppressed-output-round2-fixes FR7); this is the delivery side's
/// name for it.
pub(in crate::mux) type ReconstructedOsc = RecoveredOsc;

/// Delivery-side name for [`recover_osc`] (see [`ReconstructedOsc`]).
pub(in crate::mux) fn reconstruct_osc_number_and_data(body: &[u8]) -> ReconstructedOsc {
    recover_osc(body)
}

/// FR2 (h)/as-01: whether the theme would answer this OSC dispatch with at
/// least one response (`src-tauri/src/render/theme.rs`). Only OSC 4/10/11/12
/// route to the theme's responder (`ThemeColorResponder::respond`,
/// `callbacks.rs`) — any other number, or one whose accumulation overflowed
/// term_core's `u16` (as-06), can never route there and is never a query.
pub(in crate::mux) fn is_color_query(osc: &ReconstructedOsc) -> bool {
    match osc.number {
        Some(4) => osc_4_has_query_pair(&osc.data),
        Some(n @ (10 | 11 | 12)) => osc_default_color_has_query_item(&osc.data, n),
        _ => false,
    }
}

/// OSC 4 (`Theme::apply_palette_set`): `index;spec[;index;spec...]` pairs.
/// The index is parsed the same way (`trim().parse::<usize>()`) and must be
/// below 256; a pair whose trimmed spec is `?` is a query.
fn osc_4_has_query_pair(data: &str) -> bool {
    let mut tokens = data.split(';');
    while let Some(index_str) = tokens.next() {
        let Some(spec_str) = tokens.next() else { break };
        let Ok(index) = index_str.trim().parse::<usize>() else {
            continue;
        };
        if index >= 256 {
            continue;
        }
        if spec_str.trim() == "?" {
            return true;
        }
    }
    false
}

/// OSC 10/11/12 (`Theme::apply_default_color_set`): `;`-separated items map
/// to consecutive targets starting at `osc_number`, stopping once the
/// target exceeds 12. An item whose trimmed value is `?` is a query.
fn osc_default_color_has_query_item(data: &str, osc_number: u16) -> bool {
    for (offset, item) in data.split(';').enumerate() {
        let target = u32::from(osc_number) + offset as u32;
        if target > 12 {
            break;
        }
        if item.trim() == "?" {
            return true;
        }
    }
    false
}

/// FR7/D3: whether this OSC dispatch is a viewer-launch — a complete OSC
/// 777 `emterm;<kind>;...` where `<kind>` is one of the viewer kinds the GUI
/// actually opens a child window for, or a complete OSC 9999
/// `emterm-md[;...]`.
///
/// The delivery side's selection over the shared identification
/// ([`identify_osc`]): every viewer launch except `image`, and Markdown
/// launches. Never an agent-status report, never a not-identified OSC.
///
/// D3 finding: `image` is excluded even though it is listed in the
/// viewer-kind SSOT ([`crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS`]) — the
/// GUI's `ViewerRouter::route` treats it as a reserved, not-yet-implemented
/// no-op (`src-tauri/src/viewer/mod.rs`, the `"image"` arm), so it never
/// opens a viewer window. The `agent-status` kind is handled by the daemon
/// itself and is not a viewer launch, so it is excluded by its identity.
pub(in crate::mux) fn is_viewer_launch(osc: &ReconstructedOsc) -> bool {
    match identify_osc(osc) {
        OscIdentity::ViewerLaunch(kind) => kind != "image",
        OscIdentity::MarkdownLaunch => true,
        OscIdentity::AgentStatusReport | OscIdentity::NotIdentified => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstruct_osc_number_and_data_drops_only_the_first_semicolon() {
        let osc = reconstruct_osc_number_and_data(b"10;?;?");
        assert_eq!(osc.number, Some(10));
        assert_eq!(osc.data, "?;?");
    }

    #[test]
    fn reconstruct_osc_number_and_data_leading_zeros_accumulate_normally() {
        let osc = reconstruct_osc_number_and_data(b"010;?");
        assert_eq!(osc.number, Some(10));
        assert_eq!(osc.data, "?");
    }

    #[test]
    fn reconstruct_osc_number_and_data_overflow_never_panics_and_yields_no_route() {
        // as-06: an accumulation that would overflow term_core's u16 can
        // never reliably establish a route — never a fabricated query.
        let osc = reconstruct_osc_number_and_data(b"999999999;?");
        assert_eq!(osc.number, None);
    }

    // ---- round-2 task0001 (FR7): delivery-side viewer-launch predicate ----

    fn launch_predicate(body: &[u8]) -> bool {
        is_viewer_launch(&reconstruct_osc_number_and_data(body))
    }

    /// AC-4: true for every viewer kind except `image` (canonical,
    /// leading-zero and non-digit-prefixed forms) and for Markdown launches;
    /// false for `image`, agent-status and everything not identified.
    #[test]
    fn round2_task0001_viewer_launch_predicate_selects_deliverable_launches_in_all_forms() {
        for &kind in crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS {
            let forms = [
                format!("777;emterm;{kind};x"),
                format!("777;emterm;{kind}"),
                format!("0777;emterm;{kind};x"),
                format!("777emterm;;{kind};x"),
            ];
            for body in forms {
                assert_eq!(
                    launch_predicate(body.as_bytes()),
                    kind != "image",
                    "body {body:?}"
                );
            }
        }
        for body in [
            "9999;emterm-md",
            "9999;emterm-md;x",
            "09999;emterm-md;x",
            "09999;emterm-md",
        ] {
            assert!(launch_predicate(body.as_bytes()), "body {body:?}");
        }
        for body in [
            "777;emterm;agent-status;x",
            "0777;emterm;agent-status;x",
            "777;emterm;fold;x",
            "9999;emterm-mux;x",
            "9999;emterm-mdx",
            "777;other;x",
            "778;emterm;markdown;x",
            "70000;emterm;markdown;x",
            "10;?",
        ] {
            assert!(!launch_predicate(body.as_bytes()), "body {body:?}");
        }
    }

    fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }

    /// Ring-written bytes of a chunk the way the reader produces them: the
    /// write filter's strip over the main-buffer span.
    fn ring_bytes(chunk: &[u8], ring_written: &std::ops::Range<usize>) -> Vec<u8> {
        crate::mux::scrollback_filter::strip_pty_output_for_scrollback_write(
            &chunk[ring_written.clone()],
        )
    }

    /// AC-5 (TS-7, registry): a viewer launch written with a leading-zero
    /// number reaches the client exactly once — absent from the ring bytes
    /// the snapshot is assembled from, present once in the replacement.
    #[test]
    fn round2_a93dffe3_leading_zero_viewer_launch_reaches_the_client_once() {
        use crate::mux::ipc::pty_spawn::suppressed_output::build_suppressed_replacement;

        // Main-screen chunks: the whole chunk is ring-written.
        for launch in [
            b"\x1b]0777;emterm;markdown;begin\x07".as_slice(),
            b"\x1b]09999;emterm-md;begin\x1b\\".as_slice(),
        ] {
            let mut chunk = b"before".to_vec();
            chunk.extend_from_slice(launch);
            chunk.extend_from_slice(b"after");
            let ring_written = 0..chunk.len();

            let in_ring = count_occurrences(&ring_bytes(&chunk, &ring_written), launch);
            let in_replacement = count_occurrences(
                &build_suppressed_replacement(&chunk, &[ring_written], &[], &[]),
                launch,
            );
            assert_eq!(
                in_ring,
                0,
                "launch {:?} must be stripped from the ring",
                String::from_utf8_lossy(launch)
            );
            assert_eq!(
                in_replacement,
                1,
                "launch {:?} must be delivered once",
                String::from_utf8_lossy(launch)
            );
            assert_eq!(in_ring + in_replacement, 1);
        }

        // Alternate-screen range: nothing of the launch is ring-written, the
        // replacement delivers it once.
        for launch in [
            b"\x1b]0777;emterm;markdown;begin\x07".as_slice(),
            b"\x1b]09999;emterm-md;begin\x1b\\".as_slice(),
        ] {
            let mut chunk = b"main\x1b[?1049h".to_vec();
            let main_end = chunk.len();
            chunk.extend_from_slice(launch);
            chunk.extend_from_slice(b"alt text");
            let ring_written = 0..main_end;

            let in_ring = count_occurrences(&ring_bytes(&chunk, &ring_written), launch);
            let in_replacement = count_occurrences(
                &build_suppressed_replacement(&chunk, &[ring_written], &[], &[]),
                launch,
            );
            assert_eq!(in_ring, 0);
            assert_eq!(
                in_replacement,
                1,
                "alternate-screen launch {:?} must be delivered once",
                String::from_utf8_lossy(launch)
            );
        }

        // Image launch: stripped from the ring, never delivered.
        let launch = b"\x1b]0777;emterm;image;begin\x07".as_slice();
        let mut chunk = b"before".to_vec();
        chunk.extend_from_slice(launch);
        chunk.extend_from_slice(b"after");
        let ring_written = 0..chunk.len();
        let in_ring = count_occurrences(&ring_bytes(&chunk, &ring_written), launch);
        let in_replacement = count_occurrences(
            &build_suppressed_replacement(&chunk, &[ring_written], &[], &[]),
            launch,
        );
        assert_eq!(in_ring, 0, "image launch must be stripped from the ring");
        assert_eq!(in_replacement, 0, "image launch is never delivered");
    }

    /// AC-1 (FR2 (g)/(h), as-04): [`is_color_query`] must agree with the
    /// REAL theme's routing (`ThemeColorResponder::respond`,
    /// `crate::callbacks`) on every OSC number/payload combination — the
    /// predicate exists purely to decide whether to forward an OSC
    /// verbatim, so it must never say "not a query" when the live client
    /// would actually answer it, and never say "query" when the client
    /// would stay silent.
    #[cfg(feature = "gui")]
    #[test]
    fn color_query_predicate_matches_client_osc_number_and_theme_rules() {
        use crate::callbacks::{NativeCallbackState, ThemeColorResponder};
        use crate::render::theme::Theme;
        use parking_lot::Mutex;
        use std::sync::Arc;
        use term_core::{OscResponder, OscTerminator};

        let theme = Arc::new(Mutex::new(Theme::default()));
        let state = Arc::new(Mutex::new(NativeCallbackState::default()));
        let responder = ThemeColorResponder::new(theme, state);

        let cases: &[(u16, &str)] = &[
            (4, "1;?;2;#000000"), // OSC 4: a query pair present among others
            (4, "1;#000000"),     // OSC 4: set only, no query pair
            (4, "256;?"),         // OSC 4: index out of range, never a query
            (10, "?"),
            (10, "#fff"),
            (10, "#fff;?"), // set then query — routes on the query item
            (11, "?"),
            (12, "?"),
            (9, "some;text"), // not one of 4/10/11/12 at all
            (13, "?"),        // adjacent to but outside 10..=12
        ];
        for &(number, data) in cases {
            let osc = ReconstructedOsc {
                number: Some(number),
                data: data.to_string(),
            };
            let predicted = is_color_query(&osc);
            let actual_has_response = !responder
                .respond(number, data, OscTerminator::Bel)
                .is_empty();
            assert_eq!(
                predicted, actual_has_response,
                "OSC {number};{data}: predicate disagreed with the theme's actual routing"
            );
        }
    }

    // ---- round-2 FR1: designator-slot ESCs are never restart positions ----

    const ESC: u8 = 0x1b;

    /// A window of exactly [`RETAINED_WINDOW_BYTES`]: `prefix` followed by
    /// `fill` bytes.
    fn full_window(prefix: &[u8], fill: u8) -> Vec<u8> {
        assert!(prefix.len() <= RETAINED_WINDOW_BYTES);
        let mut window = prefix.to_vec();
        window.resize(RETAINED_WINDOW_BYTES, fill);
        assert_eq!(window.len(), RETAINED_WINDOW_BYTES);
        window
    }

    /// A full window made of the `ESC <intro> ESC <intro> …` chain, cut at
    /// exactly [`RETAINED_WINDOW_BYTES`].
    fn designator_chain_window(intro: u8) -> Vec<u8> {
        (0..RETAINED_WINDOW_BYTES)
            .map(|i| if i % 2 == 0 { ESC } else { intro })
            .collect()
    }

    /// AC-1 (FR1, TM-1): with a full window, the ESC at offset 0 is never a
    /// restart position — whether it is the designator byte of an `ESC (`
    /// that began before the window cannot be decided.
    #[test]
    fn ac1_derive_prefix_start_never_returns_offset_zero() {
        let window = full_window(&[ESC], b'x');
        assert_eq!(
            derive_prefix_start(&window),
            window.len(),
            "no other ESC exists, so the as-05 fallback (ground at the window end) applies"
        );
    }

    /// AC-1 (FR1): an ESC at offset 1 right after a `(` / `)` at offset 0 is
    /// excluded (the pre-existing rule, kept).
    #[test]
    fn ac1_derive_prefix_start_excludes_esc_at_offset_one_after_an_introducer_at_offset_zero() {
        for intro in [b'(', b')'] {
            let window = full_window(&[intro, ESC], b'x');
            assert_eq!(
                derive_prefix_start(&window),
                window.len(),
                "intro {:?}: ESC at offset 1 may be a designator byte",
                intro as char
            );
        }
    }

    /// AC-1 (FR1, TM-1): an ESC at offset `i >= 2` preceded by `(` / `)`
    /// that is itself preceded by ESC sits in a designator slot and is
    /// excluded.
    #[test]
    fn ac1_derive_prefix_start_excludes_an_esc_in_a_designator_slot_past_offset_one() {
        for intro in [b'(', b')'] {
            let window = full_window(&[ESC, intro, ESC], b'x');
            assert_eq!(
                derive_prefix_start(&window),
                window.len(),
                "intro {:?}: ESC at offset 2 follows `ESC {:?}` and is the designator byte",
                intro as char,
                intro as char
            );
        }
    }

    /// AC-1 (FR1): the rule excludes only what it names — an ESC preceded by
    /// `(` where the byte before that is NOT ESC is a fresh escape and stays
    /// a valid restart position, and so does an ESC after any other byte.
    #[test]
    fn ac1_derive_prefix_start_keeps_esc_that_is_not_in_a_designator_slot() {
        // `x ( ESC …`: the `(` is plain text, the ESC starts a sequence.
        let window = full_window(&[b'x', b'(', ESC], b'y');
        assert_eq!(derive_prefix_start(&window), 2);
        // `ESC ( ESC z ESC …`: ESC at 0 and the designator ESC at 2 are
        // excluded; the ESC at 4 follows a plain byte.
        let window = full_window(&[ESC, b'(', ESC, b'z', ESC], b'y');
        assert_eq!(derive_prefix_start(&window), 4);
        // `ESC ESC …`: the second ESC follows an ESC, not an introducer —
        // it is a fresh decision point (FR2 (e)).
        let window = full_window(&[ESC, ESC], b'y');
        assert_eq!(derive_prefix_start(&window), 1);
    }

    /// AC-1 (FR1): every ESC of an `ESC ( ESC ( …` chain is excluded in
    /// turn, so a chain-only window falls back exactly as before; a chain
    /// followed by a plain byte and an ESC restarts at that later ESC.
    #[test]
    fn ac1_derive_prefix_start_excludes_every_esc_of_a_designator_chain() {
        for intro in [b'(', b')'] {
            let window = designator_chain_window(intro);
            assert_eq!(
                derive_prefix_start(&window),
                window.len(),
                "intro {:?}: every ESC of the chain is excluded",
                intro as char
            );
        }
        for pairs in [1usize, 2, 3, 60, 61] {
            let mut prefix: Vec<u8> = [ESC, b'('].repeat(pairs);
            prefix.push(b'z');
            prefix.push(ESC);
            let window = full_window(&prefix, b'y');
            assert_eq!(
                derive_prefix_start(&window),
                pairs * 2 + 1,
                "{pairs} chain pairs, then a plain byte, then a decidable ESC"
            );
        }
    }

    /// AC-1 (FR1, as-05): a window with no qualifying ESC falls back exactly
    /// as before — ground at the window end, or the start of a trailing
    /// incomplete UTF-8 character.
    #[test]
    fn ac1_derive_prefix_start_fallback_is_unchanged_when_no_esc_qualifies() {
        let window = full_window(&[ESC, b'(', ESC], b'x');
        assert_eq!(derive_prefix_start(&window), window.len());

        let mut window = full_window(&[ESC, b'(', ESC], b'x');
        let last = window.len() - 1;
        window[last] = 0xe4; // 3-byte UTF-8 lead, incomplete
        assert_eq!(
            derive_prefix_start(&window),
            last,
            "a trailing UTF-8 partial starts the walk at its lead byte"
        );
    }

    /// AC-1 (FR1): a window shorter than the cap still holds the whole
    /// stream and is scanned from offset 0, whatever it starts with.
    #[test]
    fn ac1_derive_prefix_start_scans_a_short_window_from_its_start() {
        let mut window = vec![ESC, b'(', ESC, b']', b'1', b'0', b';'];
        window.resize(RETAINED_WINDOW_BYTES - 1, b' ');
        assert_eq!(derive_prefix_start(&window), 0);
        assert_eq!(derive_prefix_start(&[]), 0);
    }

    /// AC-1 (TM-2): windows made of all ESC bytes, all `(` bytes, or
    /// alternating `ESC (` complete in one pass without panicking, for the
    /// window alone and through the whole scan.
    #[test]
    fn ac1_degenerate_full_windows_complete_without_panicking() {
        let all_esc = vec![ESC; RETAINED_WINDOW_BYTES];
        let all_paren = vec![b'('; RETAINED_WINDOW_BYTES];
        let alternating = designator_chain_window(b'(');
        let alternating_odd: Vec<u8> = alternating.iter().skip(1).copied().collect();
        assert_eq!(derive_prefix_start(&all_esc), 1);
        assert_eq!(derive_prefix_start(&all_paren), RETAINED_WINDOW_BYTES);
        assert_eq!(derive_prefix_start(&alternating), RETAINED_WINDOW_BYTES);
        // 255 bytes: shorter than the cap, scanned from the start.
        assert_eq!(derive_prefix_start(&alternating_odd), 0);

        let chunk = b"\x1b]10;?\x07\x1b[6n";
        for window in [&all_esc, &all_paren, &alternating] {
            let outcome = scan(window, chunk, &[]);
            assert_eq!(outcome.combined.len() - outcome.boundary, chunk.len());
        }
    }

    /// AC-3 (FR1 positive): when the full window holds a decidable ESC, the
    /// scan starts there and the color query the window's OSC leads into is
    /// reported exactly once, verbatim.
    #[test]
    fn ac3_scan_starts_at_a_decidable_esc_in_a_full_window() {
        // `x`, then `ESC ]10;`, padding to 256 bytes; the chunk completes
        // the query.
        let window = full_window(&[b'x', ESC, b']', b'1', b'0', b';'], b' ');
        let chunk = b"?\x07";
        assert_eq!(derive_prefix_start(&window), 1);
        let outcome = scan(&window, chunk, &[]);
        assert_eq!(
            outcome.items.len(),
            1,
            "exactly one item: {:?}",
            outcome.items
        );
        let item = &outcome.items[0];
        assert_eq!(item.kind, ScanItemKind::ColorQuery);
        let mut expected = window[1..].to_vec();
        expected.extend_from_slice(chunk);
        assert_eq!(&outcome.combined[item.range.clone()], expected.as_slice());
    }

    /// AC-3 (FR1): a window shorter than the cap is scanned from its start
    /// even when it begins with the bytes a full window would exclude.
    #[test]
    fn ac3_scan_of_a_short_window_starts_at_its_first_byte() {
        // `ESC ]10;` at offset 0 of a 7-byte window: a full window would
        // never restart at offset 0, a short one holds the whole stream.
        let window = [ESC, b']', b'1', b'0', b';', b' ', b' '];
        let chunk = b"?\x07";
        let outcome = scan(&window, chunk, &[]);
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(outcome.items[0].kind, ScanItemKind::ColorQuery);
        assert_eq!(outcome.items[0].range.start, 0);
    }

    // ---- mux-suppressed-output-round2-fixes FR5: excluded pieces ----

    #[test]
    fn scan_skips_an_item_overlapping_an_excluded_piece_and_keeps_the_others() {
        let chunk = b"\x1b[6n\x1b[c";
        let outcome = scan(&[], chunk, &[0..4]);
        assert_eq!(
            outcome.items,
            vec![ScanItem {
                kind: ScanItemKind::CsiQuery,
                range: 4..7
            }]
        );
    }

    #[test]
    fn scan_finds_an_item_in_the_gap_between_two_excluded_pieces() {
        // `ESC ] 2 ; x`, `ESC [ 6 n`, `y`: the pieces are the two main ranges.
        let chunk = b"\x1b]2;x\x1b[6ny";
        let outcome = scan(&[], chunk, &[0..5, 9..10]);
        assert_eq!(
            outcome.items,
            vec![ScanItem {
                kind: ScanItemKind::CsiQuery,
                range: 5..9
            }]
        );
    }

    #[test]
    fn scan_reports_no_tail_when_a_piece_is_excluded_and_a_tail_otherwise() {
        let chunk = b"ok\x1b[3";
        assert_eq!(scan(&[], chunk, &[]).tail, Some(2..chunk.len()));
        assert_eq!(scan(&[], chunk, &[2..chunk.len()]).tail, None);
    }

    // ---- mux-suppressed-output-round3-fixes task0003 (FR5): the as-05
    // fallback with an undecidable trailing designator introducer ----

    /// A full window of `ESC (` / `ESC )` pairs that alternate between the
    /// two introducers, cut at exactly [`RETAINED_WINDOW_BYTES`].
    fn mixed_designator_chain_window() -> Vec<u8> {
        (0..RETAINED_WINDOW_BYTES)
            .map(|i| match (i % 2, (i / 2) % 2) {
                (0, _) => ESC,
                (_, 0) => b'(',
                _ => b')',
            })
            .collect()
    }

    /// `(` at window offset 0 followed by `ESC <intro>` pairs. The window is
    /// [`RETAINED_WINDOW_BYTES`] + 1 bytes long: at exactly the retention
    /// size a window that starts with `(`, holds no decidable ESC and ends
    /// in `ESC <intro>` cannot exist (the chain's ESCs would sit at odd
    /// offsets and the last byte, at an odd offset, would be an ESC).
    /// `derive_prefix_start` treats any window of at least the retention
    /// size as full.
    fn offset_zero_paren_chain_window(intro: u8) -> Vec<u8> {
        std::iter::once(b'(')
            .chain([ESC, intro].repeat(RETAINED_WINDOW_BYTES / 2))
            .collect()
    }

    /// The five chunk shapes of AC-1, each with a construct (or incomplete
    /// construct) that starts at the chunk's first byte.
    fn construct_at_first_byte_chunks() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("OSC 11 color query", b"\x1b]11;?\x07".to_vec()),
            ("CSI 6n query", b"\x1b[6n".to_vec()),
            (
                "OSC 777 markdown launch",
                b"\x1b]777;emterm;markdown;begin;id=r3\x07".to_vec(),
            ),
            ("incomplete CSI", b"\x1b[".to_vec()),
            ("incomplete OSC", b"\x1b]11;?".to_vec()),
        ]
    }

    /// A full window in ground state (no ESC at all): the scan starts at the
    /// chunk and nothing is undecided.
    fn ground_window() -> Vec<u8> {
        vec![b'x'; RETAINED_WINDOW_BYTES]
    }

    /// AC-1 (FR5, TM-1): a full window with no decidable ESC that ends in
    /// `ESC (` / `ESC )` leaves the chunk's first byte possibly consumed as a
    /// designator, so nothing is extracted — no item and no tail — from a
    /// construct starting at that byte. The same chunks yield an item or a
    /// tail from a ground window (the controls), so the rule is what removes
    /// them.
    #[test]
    fn round3_as05_trailing_designator_introducer_extracts_nothing_at_the_chunk_start() {
        use crate::mux::ipc::pty_spawn::suppressed_output::build_suppressed_replacement;

        let windows: Vec<(&str, Vec<u8>)> = vec![
            ("ESC ( chain", designator_chain_window(b'(')),
            ("ESC ) chain", designator_chain_window(b')')),
            ("mixed chain", mixed_designator_chain_window()),
            (
                "( at offset 0, ESC ( chain",
                offset_zero_paren_chain_window(b'('),
            ),
            (
                "( at offset 0, ESC ) chain",
                offset_zero_paren_chain_window(b')'),
            ),
        ];
        for (chunk_label, chunk) in construct_at_first_byte_chunks() {
            let control = scan(&ground_window(), &chunk, &[]);
            assert!(
                !control.items.is_empty() || control.tail.is_some(),
                "{chunk_label}: the control scan from ground must yield an item or a tail"
            );
            for (window_label, window) in &windows {
                let outcome = scan(window, &chunk, &[]);
                assert_eq!(
                    outcome.boundary, 0,
                    "{window_label} / {chunk_label}: the as-05 fallback starts at the window's end"
                );
                assert!(
                    outcome.items.is_empty(),
                    "{window_label} / {chunk_label}: no item may start at the chunk's first byte, got {:?}",
                    outcome.items
                );
                assert_eq!(
                    outcome.tail, None,
                    "{window_label} / {chunk_label}: no tail may start at the chunk's first byte"
                );
                assert!(
                    build_suppressed_replacement(&chunk, &[0..chunk.len()], &[], window).is_empty(),
                    "{window_label} / {chunk_label}: nothing is delivered for the chunk"
                );
            }
        }
    }

    /// AC-1 (FR5): a construct that starts later in the chunk, after a
    /// construct that ends inside it, is classified by the existing rules
    /// exactly as from a ground window — only what starts at the chunk's
    /// first byte is dropped.
    #[test]
    fn round3_as05_trailing_designator_introducer_keeps_constructs_that_start_later() {
        let window = designator_chain_window(b'(');
        let cases: Vec<(
            &str,
            &[u8],
            Vec<(ScanItemKind, Range<usize>)>,
            Option<Range<usize>>,
        )> = vec![
            (
                "OSC title, then CSI query",
                b"\x1b]2;x\x07\x1b[6n",
                vec![(ScanItemKind::CsiQuery, 6..10)],
                None,
            ),
            (
                "CSI query, then OSC 11 query",
                b"\x1b[6n\x1b]11;?\x07",
                vec![(ScanItemKind::ColorQuery, 4..11)],
                None,
            ),
            (
                "ESC ESC, then OSC 11 query",
                b"\x1b\x1b]11;?\x07",
                vec![(ScanItemKind::ColorQuery, 1..8)],
                None,
            ),
            (
                "designator pair, then CSI query",
                b"\x1b(A\x1b[6n",
                vec![(ScanItemKind::CsiQuery, 3..7)],
                None,
            ),
            (
                "CSI query, then an incomplete CSI",
                b"\x1b[6n\x1b[3",
                vec![],
                Some(4..7),
            ),
        ];
        for (label, chunk, expected_items, expected_tail) in cases {
            let outcome = scan(&window, chunk, &[]);
            assert_eq!(outcome.boundary, 0, "{label}: fallback start");
            let items: Vec<(ScanItemKind, Range<usize>)> = outcome
                .items
                .iter()
                .map(|item| (item.kind, item.range.clone()))
                .collect();
            assert_eq!(items, expected_items, "{label}: items");
            assert_eq!(outcome.tail, expected_tail, "{label}: tail");

            // The same chunk from a ground window, minus what starts at byte 0.
            let control = scan(&ground_window(), chunk, &[]);
            let control_items: Vec<(ScanItemKind, Range<usize>)> = control
                .items
                .iter()
                .filter(|item| item.range.start != 0)
                .map(|item| (item.kind, item.range.clone()))
                .collect();
            assert_eq!(
                items, control_items,
                "{label}: items equal the ground scan's"
            );
            let control_tail = control.tail.filter(|tail| tail.start != 0);
            assert_eq!(
                outcome.tail, control_tail,
                "{label}: tail equals the ground scan's"
            );
        }
    }

    /// AC-1 / AC-2 (FR5, unaffected case): the rule removes control sequences
    /// that start at the chunk's first byte. An incomplete UTF-8 character is
    /// not a control sequence, so its tail is kept at the chunk's first byte
    /// as well as later (re-delivering it reproduces the client's handling of
    /// the byte whether or not it was the designator).
    #[test]
    fn round3_as05_trailing_designator_introducer_keeps_an_incomplete_utf8_tail() {
        let window = designator_chain_window(b'(');
        assert_eq!(scan(&window, &[0xe4, 0xb8], &[]).tail, Some(0..2));
        assert_eq!(
            scan(&ground_window(), &[0xe4, 0xb8], &[]).tail,
            Some(0..2),
            "control: a ground window yields the same tail"
        );
        assert_eq!(scan(&window, b"a\xe4", &[]).tail, Some(1..2));
    }

    /// AC-2 (FR5, unaffected cases): the trailing `(` / `)` of a full
    /// fallback window is not preceded by ESC, so it is a designator byte or
    /// text and the scan still starts at the chunk.
    #[test]
    fn round3_as05_trailing_paren_not_preceded_by_esc_still_scans_from_the_chunk_start() {
        let query = b"\x1b[6n";
        let mut windows: Vec<(&str, Vec<u8>)> = vec![
            ("( bytes only", vec![b'('; RETAINED_WINDOW_BYTES]),
            (") bytes only", vec![b')'; RETAINED_WINDOW_BYTES]),
        ];
        for intro in [b'(', b')'] {
            let mut window = ground_window();
            let last = window.len() - 1;
            window[last] = intro;
            windows.push(("x then a trailing paren", window));
        }
        for (label, window) in windows {
            let outcome = scan(&window, query, &[]);
            assert_eq!(
                outcome.items,
                vec![ScanItem {
                    kind: ScanItemKind::CsiQuery,
                    range: 0..4
                }],
                "{label}: the query at the chunk start is extracted"
            );
        }
    }

    /// AC-2 (FR5, unaffected cases): a window that holds a decidable ESC
    /// restarts there as before, even though it ends in `ESC (` — the walk
    /// reproduces the client's state, so a construct at the chunk's first
    /// byte is classified by the walk itself (here the chunk's first byte is
    /// the designator the trailing `ESC (` awaits, so the query is not one).
    #[test]
    fn round3_as05_window_with_a_decidable_esc_keeps_its_results() {
        // `ESC [ 0 m` early, then padding, then `ESC ( ESC (`: the early ESC
        // is decidable, so the restart position is there and the trailing
        // `ESC (` does not trigger the rule.
        let mut prefix = vec![b'x', ESC, b'[', b'0', b'm'];
        prefix.resize(RETAINED_WINDOW_BYTES - 4, b'y');
        prefix.extend_from_slice(&[ESC, b'(', ESC, b'(']);
        let ground_ending = full_window(&prefix, b'y');
        assert_eq!(ground_ending.len(), RETAINED_WINDOW_BYTES);
        // The walk from the decidable ESC ends in ground: a query at the
        // chunk's first byte is extracted.
        let outcome = scan(&ground_ending, b"\x1b]11;?\x07", &[]);
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(outcome.items[0].kind, ScanItemKind::ColorQuery);

        // Same window with a single trailing `ESC (`: the walk ends awaiting
        // the designator, which is the chunk's first byte: no query there.
        let mut awaiting = full_window(&[b'x', ESC, b'[', b'0', b'm'], b'y');
        let last = awaiting.len();
        awaiting[last - 2] = ESC;
        awaiting[last - 1] = b'(';
        let outcome = scan(&awaiting, b"\x1b]11;?\x07", &[]);
        assert!(
            outcome.items.is_empty(),
            "the chunk's ESC is the designator: {:?}",
            outcome.items
        );
    }

    /// AC-2 (FR5, unaffected cases): a window shorter than the retention
    /// size is walked from its start, so a trailing `ESC (` that is itself
    /// preceded by a designator-slot ESC leaves the walk in ground and a
    /// query at the chunk's first byte is extracted.
    #[test]
    fn round3_as05_short_window_is_walked_from_its_start() {
        let mut window = vec![b'x'; RETAINED_WINDOW_BYTES - 5];
        window.extend_from_slice(&[ESC, b'(', ESC, b'(']);
        assert_eq!(window.len(), RETAINED_WINDOW_BYTES - 1);
        let outcome = scan(&window, b"\x1b]11;?\x07", &[]);
        assert_eq!(outcome.items.len(), 1);
        assert_eq!(outcome.items[0].kind, ScanItemKind::ColorQuery);
        assert_eq!(outcome.items[0].range.start, outcome.boundary);
    }

    /// AC-4 (TM-2, NFR5, TS-9): degenerate full windows combined with
    /// adversarial chunks finish within the budget and never panic, through
    /// the scan and through the replacement builder.
    #[test]
    fn round3_as05_degenerate_windows_with_adversarial_chunks_finish_within_budget() {
        use crate::mux::ipc::pty_spawn::suppressed_output::build_suppressed_replacement;

        let mut paren_then_text = vec![b'('];
        paren_then_text.resize(RETAINED_WINDOW_BYTES, b'x');
        let windows: Vec<(&str, Vec<u8>)> = vec![
            ("all (", vec![b'('; RETAINED_WINDOW_BYTES]),
            ("all ESC", vec![ESC; RETAINED_WINDOW_BYTES]),
            ("ESC ( chain", designator_chain_window(b'(')),
            ("ESC ) chain", designator_chain_window(b')')),
            ("mixed chain", mixed_designator_chain_window()),
            ("( then non-ESC bytes", paren_then_text),
            (
                "( at offset 0, ESC ( chain",
                offset_zero_paren_chain_window(b'('),
            ),
        ];
        const BIG: usize = 64 * 1024;
        let mut long_csi = vec![ESC, b'['];
        long_csi.resize(BIG, b'1');
        let mut long_osc = vec![ESC, b']'];
        long_osc.resize(BIG, b'x');
        let mut long_osc_terminated = long_osc.clone();
        long_osc_terminated.push(0x07);
        let esc_paren_chain = [ESC, b'('].repeat(BIG / 2);
        let chunks: Vec<(&str, Vec<u8>)> = vec![
            ("long CSI", long_csi),
            ("long OSC", long_osc),
            ("long terminated OSC", long_osc_terminated),
            ("64 KiB ESC ( chain", esc_paren_chain),
        ];

        let start = std::time::Instant::now();
        for (window_label, window) in &windows {
            for (chunk_label, chunk) in &chunks {
                let outcome = scan(window, chunk, &[]);
                assert_eq!(
                    outcome.combined.len() - outcome.boundary,
                    chunk.len(),
                    "{window_label} / {chunk_label}"
                );
                let _ = build_suppressed_replacement(chunk, &[0..chunk.len()], &[], window);
            }
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "degenerate windows with adversarial chunks must finish within the budget; took {:?}",
            start.elapsed()
        );
    }
}
