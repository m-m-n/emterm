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
//! as that ESC is not itself sitting in an undecidable designator slot,
//! which is exactly what the offset-0 / offset-1-after-`(`/`)` exclusions
//! rule out.

use std::ops::Range;

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
/// limit without resolving one.
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
    /// The trailing incomplete construct at the scan limit, if any. May
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
/// up to [`RETAINED_WINDOW_BYTES`]). `limit_in_chunk` bounds how far into
/// `chunk` the scan is allowed to look (`<= chunk.len()`); passing anything
/// less than `chunk.len()` disables tail discovery for this call (FR4: used
/// when the write filter's pending run — not this scan — owns the tail, so
/// the bytes it will re-deliver must never also be found here).
///
/// Contract (NFR5/TM-2): a single forward pass, work proportional to
/// `window.len() + chunk.len()`; never panics.
pub(in crate::mux) fn scan(window: &[u8], chunk: &[u8], limit_in_chunk: usize) -> ScanOutcome {
    let s = derive_prefix_start(window);
    let mut combined = Vec::with_capacity((window.len() - s) + chunk.len());
    combined.extend_from_slice(&window[s..]);
    let boundary = combined.len();
    combined.extend_from_slice(chunk);

    let limit = boundary + limit_in_chunk.min(chunk.len());
    let (raw_items, tail_start, tail_strip_c0) = scan_view(&combined, limit);

    let items = raw_items
        .into_iter()
        .filter(|item| item.range.end > boundary)
        .collect();
    let tail = tail_start.map(|start| start..combined.len());

    ScanOutcome {
        combined,
        boundary,
        items,
        tail,
        tail_strip_c0,
    }
}

/// FR6 "Full window" / D2 "stream shorter than N": pick the offset within
/// `window` from which a plain Ground-state walk (through `window[s..]`
/// then the chunk) reproduces the true parser's state — see the module doc
/// for why any ESC except the designator slot is a safe resync point.
fn derive_prefix_start(window: &[u8]) -> usize {
    if window.len() < RETAINED_WINDOW_BYTES {
        // D2: the window holds the entire stream so far — no truncation,
        // no ambiguity. Walk it from the very start.
        return 0;
    }
    let mut i = 0;
    while i < window.len() {
        if window[i] == 0x1b {
            let undecidable = i == 0 || (i == 1 && matches!(window[0], b'(' | b')'));
            if !undecidable {
                return i;
            }
        }
        i += 1;
    }
    // as-05 fallback: no decidable ESC in the window. The state is Ground,
    // unless the window's own trailing bytes are an in-progress UTF-8
    // character — in which case start the walk there so the combined scan
    // naturally re-derives the UTF-8-partial state at the boundary.
    utf8_tail_start_at_end(window).unwrap_or(window.len())
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

/// Single forward pass over `combined[..limit]` (FR2), classifying every
/// completed OSC/CSI as a reportable item when it qualifies (FR7/FR9), and
/// reporting the trailing incomplete construct when the walk reaches
/// `limit` without resolving one AND `limit == combined.len()` (tail
/// discovery is disabled when the caller bounded `limit` below the buffer's
/// end — FR4's pending exclusion).
///
/// Returns `(items, tail_start, tail_strip_c0)`.
fn scan_view(combined: &[u8], limit: usize) -> (Vec<ScanItem>, Option<usize>, bool) {
    let detect_tail = limit == combined.len();
    let view = &combined[..limit];
    let mut items = Vec::new();
    let mut pos = 0usize;
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

    if tail_start.is_none() && detect_tail {
        tail_start = utf8_tail_start_at_end(view);
    }

    (items, tail_start, tail_strip_c0)
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::mux) struct ReconstructedOsc {
    pub(in crate::mux) number: Option<u16>,
    pub(in crate::mux) data: String,
}

pub(in crate::mux) fn reconstruct_osc_number_and_data(body: &[u8]) -> ReconstructedOsc {
    let mut acc: u32 = 0;
    let mut overflowed = false;
    let mut done = false;
    let mut data = Vec::with_capacity(body.len());
    for &b in body {
        if !done {
            if b.is_ascii_digit() {
                acc = acc.saturating_mul(10).saturating_add(u32::from(b - b'0'));
                if acc > u16::MAX as u32 {
                    overflowed = true;
                }
                continue;
            }
            if b == b';' {
                done = true;
                continue;
            }
        }
        data.push(b);
    }
    let number = if overflowed { None } else { Some(acc as u16) };
    let data = String::from_utf8(data)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    ReconstructedOsc { number, data }
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
/// D3 finding: `image` is excluded even though it is listed in the
/// viewer-kind SSOT ([`crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS`]) — the
/// GUI's `ViewerRouter::route` treats it as a reserved, not-yet-implemented
/// no-op (`src-tauri/src/viewer/mod.rs`, the `"image"` arm), so it never
/// opens a viewer window. The `agent-status` kind is handled by the daemon
/// itself and is not in the SSOT at all, so it is excluded structurally.
pub(in crate::mux) fn is_viewer_launch(osc: &ReconstructedOsc) -> bool {
    match osc.number {
        Some(777) => {
            let Some(rest) = osc.data.strip_prefix("emterm;") else {
                return false;
            };
            let kind = rest.split(';').next().unwrap_or(rest);
            kind != "image" && crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS.contains(&kind)
        }
        Some(9999) => osc.data == "emterm-md" || osc.data.starts_with("emterm-md;"),
        _ => false,
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
}
