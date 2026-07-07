//! Scrollback rich-content filtering shared by the mux IPC reattach path and
//! the session pane resume path.
//!
//! This module is the single home for [`strip_replayable_rich_content`] so the
//! session layer (`mux::session::pane`) does not have to reach into the IPC
//! layer (`mux::ipc::reattach`) for it; both depend on this shared module
//! instead.

use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;

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
    // kinds; keep `fold` (fold marks) and any other kind (status-bar, …).
    if let Some(rest) = body.strip_prefix(b"777;emterm;") {
        let kind = rest.split(|&c| c == b';').next().unwrap_or(rest);
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
/// Body grammar (matches the CSI recognition in the task design):
/// parameter bytes (`0x30..=0x3F`, which includes the private markers
/// `<=>?`), then intermediate bytes (`0x20..=0x2F`), then a final byte
/// (`0x40..=0x7E`).
///
/// Returns `Some` only for a COMPLETE CSI (a valid final byte is found) that
/// matches the strip predicate. Returns `None` for: a CSI that completes but
/// does not match (the caller preserves it byte-for-byte via its normal
/// single-byte fallback, and the main loop then pushes the rest of the
/// sequence one byte at a time — still an O(n) pass overall), an
/// unterminated CSI (buffer ends before a final byte), a CSI body containing
/// a bare ESC (aborts the candidate — the caller's single-byte fallback
/// naturally re-processes bytes up to that ESC one at a time, so "the
/// scanned prefix is preserved as-is and scanning resumes at that ESC"
/// falls out of the existing fallback without special-casing), or a byte
/// outside the CSI grammar (e.g. DEL).
fn scan_csi_device_query(bytes: &[u8], from: usize) -> Option<CsiStrip> {
    let mut j = from;
    let mut private_prefix: Option<u8> = None;
    let mut first_param: u32 = 0;
    let mut collecting_first_param = true;
    let mut param_bytes_seen = 0usize;
    let mut in_intermediates = false;
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
            0x30..=0x3f if !in_intermediates => {
                // Parameter byte (digit, ';', ':', or a private marker
                // `<=>?` — only the FIRST parameter byte is a private
                // prefix, matching term_core's CSI-entry-state behavior).
                if param_bytes_seen == 0 && matches!(b, b'<' | b'=' | b'>' | b'?') {
                    private_prefix = Some(b);
                } else if collecting_first_param && b.is_ascii_digit() {
                    first_param = first_param * 10 + u32::from(b - b'0');
                } else {
                    // ';' / ':' / a digit past the first run / a stray
                    // marker not in leading position — the leading decimal
                    // run is over either way.
                    collecting_first_param = false;
                }
                param_bytes_seen += 1;
                j += 1;
            }
            0x20..=0x2f => {
                // Intermediate byte.
                in_intermediates = true;
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

/// The strip predicate (SPEC.md FR1/FR2), mirroring the dispatch conditions
/// in `crates/term_core/src/csi_dispatch.rs`: true only for CSI forms
/// term_core answers with a device response.
///
/// | final | private prefix    | intermediates | first param | query kind |
/// |-------|--------------------|----------------|--------------|------------|
/// | `n`   | none               | none           | 5 or 6       | DSR / CPR  |
/// | `c`   | none, `?`, or `>`  | none           | any          | DA1 / DA2  |
/// | `t`   | none               | none           | 14, 16, 18   | XTWINOPS   |
/// | `p`   | `?`                | `$`            | any          | DECRPM     |
///
/// `first_param` treats an empty leading digit run as 0, matching
/// term_core's `ParamParser::get_first_or_zero`.
fn csi_is_device_query(
    private_prefix: Option<u8>,
    intermediates: &[u8],
    first_param: u32,
    final_byte: u8,
) -> bool {
    match final_byte {
        b'n' => {
            private_prefix.is_none() && intermediates.is_empty() && matches!(first_param, 5 | 6)
        }
        b'c' => {
            matches!(private_prefix, None | Some(b'?') | Some(b'>')) && intermediates.is_empty()
        }
        b't' => {
            private_prefix.is_none()
                && intermediates.is_empty()
                && matches!(first_param, 14 | 16 | 18)
        }
        b'p' => private_prefix == Some(b'?') && intermediates == [b'$'],
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
