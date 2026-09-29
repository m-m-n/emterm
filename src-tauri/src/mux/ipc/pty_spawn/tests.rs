use super::*;
use crate::mux::scrollback_filter::strip_pty_output_for_scrollback_write;
use crate::pty::visibility::HIDDEN_PASSTHROUGH_CAPACITY_MUX;
use std::borrow::Cow;
use term_core::terminal_core::ReplaySegment;

/// Convert term_core's `ReplaySegment` (used by test-side recording
/// construction) into the mux-layer's plain `(usize, u16, u16)` tuples
/// `build_snapshot_bytes` accepts.
fn to_tuples(segments: &[ReplaySegment]) -> Vec<(usize, u16, u16)> {
    segments
        .iter()
        .map(|s| (s.offset as usize, s.cols, s.rows))
        .collect()
}

/// Convert the mux-layer's plain `(usize, u16, u16)` tuples (as
/// returned by `build_snapshot_bytes`) into `ReplaySegment`s for
/// `reset_and_replay_segments`.
fn to_replay_segments(tuples: &[(usize, u16, u16)]) -> Vec<ReplaySegment> {
    tuples
        .iter()
        .map(|&(offset, cols, rows)| ReplaySegment {
            offset: offset as u32,
            cols,
            rows,
        })
        .collect()
}

// ── pane_env_vars (EMTERM_PANE_ID injection, AC-6) ─────────────────────

#[test]
fn pane_env_vars_includes_emterm_pane_id_matching_the_given_public_id() {
    let vars = pane_env_vars("abc123-7");
    let found = vars.iter().find(|(k, _)| *k == "EMTERM_PANE_ID");
    assert_eq!(found.map(|(_, v)| v.as_str()), Some("abc123-7"));
}

#[test]
fn pane_env_vars_pane_id_differs_per_pane() {
    let get = |vars: &[(&str, String)]| {
        vars.iter()
            .find(|(k, _)| *k == "EMTERM_PANE_ID")
            .unwrap()
            .1
            .clone()
    };
    let a = get(&pane_env_vars("abc123-1"));
    let b = get(&pane_env_vars("abc123-2"));
    assert_ne!(a, b, "distinct public pane ids must round-trip distinctly");
}

#[test]
fn pane_env_vars_keeps_existing_fixed_vars() {
    let vars = pane_env_vars("abc123-1");
    let get = |key: &str| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
    assert_eq!(get("TERM"), Some("xterm-256color".to_string()));
    assert_eq!(get("COLORTERM"), Some("truecolor".to_string()));
    assert_eq!(get("TERM_PROGRAM"), Some("emterm".to_string()));
    assert_eq!(get("EMTERM_MUX"), Some("1".to_string()));
}

// ── extract_main_buffer_bytes (alt-screen scrollback gating) ──────────

#[test]
fn extract_main_buffer_pure_main_borrows_whole_chunk() {
    let (bytes, alt, spans) = extract_main_buffer_bytes(b"hello \x1b[31mworld\x1b[0m", false);
    assert_eq!(&*bytes, b"hello \x1b[31mworld\x1b[0m");
    assert!(!alt);
    assert_eq!(spans, vec![0..bytes.len()]);
    assert!(
        matches!(bytes, Cow::Borrowed(_)),
        "no-toggle main chunk must borrow"
    );
}

#[test]
fn extract_main_buffer_pure_alt_yields_nothing() {
    let (bytes, alt, spans) = extract_main_buffer_bytes(b"alt frame redraw", true);
    assert!(bytes.is_empty());
    assert!(alt);
    assert!(spans.is_empty());
}

#[test]
fn extract_main_buffer_keeps_prefix_before_alt_enter() {
    // Command output then a TUI opens in the SAME read — the leading
    // main-buffer output must survive (the regression this guards).
    let (bytes, alt, spans) = extract_main_buffer_bytes(b"PRE-OUTPUT\x1b[?1049hALTUI", false);
    assert_eq!(&*bytes, b"PRE-OUTPUT");
    assert!(alt);
    assert_eq!(spans, vec![0..10]);
}

#[test]
fn extract_main_buffer_keeps_suffix_after_alt_exit() {
    let (bytes, alt, spans) = extract_main_buffer_bytes(b"ALTUI\x1b[?1049lPOST-PROMPT", true);
    assert_eq!(&*bytes, b"POST-PROMPT");
    assert!(!alt);
    assert_eq!(spans, vec![13..24]);
}

#[test]
fn extract_main_buffer_drops_balanced_internal_alt_span() {
    let (bytes, alt, spans) = extract_main_buffer_bytes(b"A\x1b[?1049hHIDDEN\x1b[?1049lB", false);
    assert_eq!(&*bytes, b"AB");
    assert!(!alt);
    assert_eq!(spans, vec![0..1, 23..24]);
}

#[test]
fn extract_main_buffer_handles_47_and_1047_forms() {
    let (b1, a1, spans1) = extract_main_buffer_bytes(b"X\x1b[?47hY", false);
    assert_eq!(&*b1, b"X");
    assert!(a1);
    assert_eq!(spans1, vec![0..1]);
    let (b2, a2, spans2) = extract_main_buffer_bytes(b"X\x1b[?1047lY", true);
    assert_eq!(&*b2, b"Y");
    assert!(!a2);
    assert_eq!(spans2, vec![9..10]);
}

// ── ScrollbackWriteFilter (stateful scrollback-write stripper) ─────────

fn new_scrollback(cap: usize) -> SharedScrollback {
    use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
    Arc::new(StdMutex::new(ScrollbackRingBuffer::new(cap)))
}

/// Feed `chunks` through a fresh [`ScrollbackWriteFilter`] and write each
/// filter output to `scrollback`, mirroring what `pty_reader_loop` does.
/// Bytes still held in the filter's `pending` at the end are NOT written
/// — matching production semantics (a still-pending unterminated
/// sequence is held until a subsequent read carries its terminator, or
/// the pending cap flushes it).
fn feed_all(scrollback: &SharedScrollback, chunks: &[&[u8]]) -> ScrollbackWriteFilter {
    let mut filter = ScrollbackWriteFilter::new();
    for chunk in chunks {
        // Dims are irrelevant to these content-shape tests; a fixed
        // value keeps them from affecting `write` (plain content, no
        // dim tracking involved).
        let (_dims, filtered) = filter.feed(chunk, (80, 24));
        if !filtered.is_empty() {
            scrollback.lock().unwrap().write(&filtered);
        }
    }
    filter
}

/// A viewer-launch OSC 777 emterm-markdown sequence delivered in a single
/// read is stripped BEFORE ever landing in the ring — so subsequent
/// overflow can't leave a headerless base64 tail behind for snapshot
/// replay.
#[test]
fn scrollback_filter_strips_osc777_markdown_viewer_in_one_feed() {
    let scrollback = new_scrollback(2048);
    let chunk = b"before\x1b]777;emterm;markdown;chunk;id=x;data=xxx\x07after";
    feed_all(&scrollback, &[chunk]);
    assert_eq!(scrollback.lock().unwrap().read_all(), b"beforeafter");
}

/// Fold marks (`777;emterm;fold;…`) and other non-viewer content are
/// preserved — the stripper only targets replayable viewer kinds.
#[test]
fn scrollback_filter_keeps_fold_and_plain_text() {
    let scrollback = new_scrollback(2048);
    let chunk = b"$ ls\r\n\x1b]777;emterm;fold;start;42\x07file.rs\r\n";
    feed_all(&scrollback, &[chunk]);
    assert_eq!(scrollback.lock().unwrap().read_all(), &chunk[..]);
}

/// Kitty graphics APC and OSC 9999 emterm-md are also viewer-adjacent
/// and must be stripped at write time (same coverage as the snapshot-time
/// stripper's SSOT).
#[test]
fn scrollback_filter_strips_kitty_and_osc9999_emterm_md() {
    let scrollback = new_scrollback(2048);
    let chunk = b"A\x1b_Gi=1;PAYLOAD\x1b\\B\x1b]9999;emterm-md;begin\x07C";
    feed_all(&scrollback, &[chunk]);
    assert_eq!(scrollback.lock().unwrap().read_all(), b"ABC");
}

/// **Regression** for the reported bug shape: `emterm markdown` emits a
/// 128 KiB payload per OSC 777 chunk while the PTY reader buffer is 64 KiB
/// (`buf = [0u8; 65536]`), so ONE viewer chunk always arrives via ≥ 2
/// `feed` calls — first with the introducer + partial base64, then with
/// the rest + terminator. The pre-fix stateless helper let both halves
/// land in the ring verbatim (the first half unterminated, the second
/// with no introducer), so ring overflow could evict the introducer while
/// the base64 tail survived and got replayed to the client on tab-switch.
/// With the stateful filter, the unterminated first half is held in
/// `pending`; the terminator arrives in the second half; the complete
/// sequence is stripped in one shot, so ring content is clean.
#[test]
fn scrollback_filter_strips_osc777_chunk_split_across_reads() {
    let scrollback = new_scrollback(256 * 1024);
    // Build the CLI-shaped OSC 777 markdown chunk: prefix + 100 KiB
    // base64-looking body + ST terminator. Total ~100 KiB, larger than a
    // 64 KiB PTY read.
    let mut full = Vec::from(&b"pre\r\n"[..]);
    full.extend_from_slice(b"\x1b]777;emterm;markdown;chunk;id=abc;seq=0;data=");
    full.extend(std::iter::repeat_n(b'A', 100_000));
    full.extend_from_slice(b"\x1b\\");
    full.extend_from_slice(b"post");
    // Split at exactly 64 KiB — the boundary a real PTY reader would use.
    let (head, tail) = full.split_at(65_536);
    let filter = feed_all(&scrollback, &[head, tail]);
    // Post-completion, `pending` should be drained back to zero.
    assert_eq!(filter.pending_len(), 0);
    let stored = scrollback.lock().unwrap().read_all();
    assert_eq!(stored, b"pre\r\npost");
}

/// The filter must hold an unterminated introducer across an *arbitrary*
/// split point (not just aligned on the reader-buffer boundary above).
/// This variant splits at the introducer's `;data=` marker and again in
/// the middle of the base64 body to exercise multiple pending / drain
/// cycles.
#[test]
fn scrollback_filter_strips_osc777_chunk_split_multi_boundary() {
    let scrollback = new_scrollback(256 * 1024);
    let mut full = Vec::from(&b"A"[..]);
    full.extend_from_slice(b"\x1b]777;emterm;image;chunk;id=x;seq=0;data=");
    full.extend(std::iter::repeat_n(b'B', 90_000));
    full.extend_from_slice(b"\x1b\\");
    full.extend_from_slice(b"Z");
    // Splits: right before `data=` payload, then midway through the body.
    let mid1 = full
        .windows(5)
        .position(|w| w == b"data=")
        .expect("data= present")
        + 5;
    let mid2 = mid1 + 40_000;
    let a = &full[..mid1];
    let b = &full[mid1..mid2];
    let c = &full[mid2..];
    let filter = feed_all(&scrollback, &[a, b, c]);
    assert_eq!(filter.pending_len(), 0);
    assert_eq!(scrollback.lock().unwrap().read_all(), b"AZ");
}

/// The pending buffer must not accept unbounded growth from a stream that
/// never terminates its introducer. Past
/// [`SCROLLBACK_FILTER_PENDING_CAP`], the filter forwards the pending
/// run raw and resets. This trades the strip guarantee for a bounded
/// per-pane memory footprint (defensive escape hatch, not a correctness
/// path — reported via warn log).
#[test]
fn scrollback_filter_flushes_raw_past_pending_cap() {
    let _scrollback = new_scrollback(2 * 1024 * 1024);
    // Start an unterminated OSC 777 introducer, then keep feeding padding
    // until pending exceeds the cap. No terminator ever arrives.
    let mut filter = ScrollbackWriteFilter::new();
    let intro = b"\x1b]777;emterm;markdown;chunk;id=x;seq=0;data=";
    let (_dims, out) = filter.feed(intro, (80, 24));
    assert!(out.is_empty(), "introducer alone is held pending");
    // Feed 520 KiB of body without a terminator; sum crosses 512 KiB.
    let padding: Vec<u8> = std::iter::repeat_n(b'A', 520 * 1024).collect();
    let (_dims, flushed) = filter.feed(&padding, (80, 24));
    // The cap escape hatch fired: pending drained raw, ring wrote the raw
    // bytes. The exact contents don't matter for correctness — the
    // invariant is that pending doesn't grow past the cap.
    assert!(!flushed.is_empty(), "cap escape hatch must emit raw bytes");
    assert_eq!(filter.pending_len(), 0);
}

/// AC-10 (task0005 rework D7'', review round-4 finding
/// `0e3f8378913e1f4a`): bytes carried across reads in `pending` are
/// attributed to the dims in effect when the run STARTED, not the dims
/// of whatever later read happened to flush them.
///
/// Confirmed to fail pre-fix: before `feed` took a `current_dims`
/// parameter, the caller (`pty_reader_loop`) unconditionally attributed
/// EVERY flush to the dims read at the top of the CURRENT call — so
/// this test's second `feed` call would have reported `dims_b`
/// `(120, 40)` instead of the expected `dims_a` `(80, 24)`.
#[test]
fn feed_attributes_a_carried_over_pending_run_to_the_dims_it_started_under() {
    let dims_a = (80u16, 24u16);
    let dims_b = (120u16, 40u16);
    let mut filter = ScrollbackWriteFilter::new();

    // A benign (non-strip-target — "fold" kind) OSC 777 left
    // unterminated in this read, produced under dims_a.
    let intro = b"\x1b]777;emterm;fold;start;42";
    let (dims_first, out_first) = filter.feed(intro, dims_a);
    assert!(
        out_first.is_empty(),
        "unterminated OSC must still be held pending"
    );
    assert_eq!(dims_first, dims_a);

    // The SECOND read carries the terminator, but the pane resized to
    // dims_b BETWEEN the two reads — the exact race the finding
    // describes (the resize's marker can win the scrollback lock ahead
    // of this flush).
    let tail = b"\x07after";
    let (dims_second, out_second) = filter.feed(tail, dims_b);
    assert_eq!(
        dims_second, dims_a,
        "the flushed bytes must be attributed to the dims the pending \
         run STARTED under (dims_a), not the dims of the read that \
         merely flushed it (dims_b)"
    );
    assert_eq!(out_second, b"\x1b]777;emterm;fold;start;42\x07after");
}

/// AC-11 (round-6 rework D7''', review round-5 finding
/// `fd379025e1900e9f`): a PARTIAL drain — read 2 completes read 1's
/// pending run AND opens a NEW unterminated one in the SAME call —
/// must attribute a LATER flush of that new run's tail to the dims of
/// the read that produced it (read 2's), not the dims of whichever run
/// started the OLDEST pending bytes (read 1's).
///
/// Confirmed to fail pre-fix: `pending_started_dims` was only reset
/// when `pending` became fully EMPTY after a drain; a partial drain
/// left it unchanged at `dims_a`, so the third `feed` below would have
/// reported `dims_a` instead of `dims_b` for content read 2 alone
/// produced.
#[test]
fn feed_reattributes_a_partial_drains_retained_tail_to_the_read_that_produced_it() {
    let dims_a = (80u16, 24u16);
    let dims_b = (120u16, 40u16);
    let dims_c = (100u16, 30u16);
    let mut filter = ScrollbackWriteFilter::new();

    // Read 1 @ dims_a: an unterminated OSC, held pending in full.
    let (dims1, out1) = filter.feed(b"\x1b]777;emterm;fold;start;1", dims_a);
    assert!(out1.is_empty());
    assert_eq!(dims1, dims_a);

    // Read 2 @ dims_b: terminates the FIRST OSC, then opens a SECOND,
    // unterminated one — a partial drain (the first OSC + "middle"
    // flush; the second OSC's start stays pending).
    let (dims2, out2) = filter.feed(b"\x07middle\x1b]777;emterm;fold;start;2", dims_b);
    assert_eq!(
        dims2, dims_a,
        "the DRAINED content (the first OSC + \"middle\") originated \
         under dims_a"
    );
    assert!(
        !out2.is_empty(),
        "test prerequisite: read 2 must actually partially drain"
    );
    assert!(
        filter.pending_len() > 0,
        "test prerequisite: read 2 must actually retain a new pending run"
    );

    // Read 3 @ dims_c: terminates the SECOND OSC. The flushed bytes
    // are entirely from read 2's own chunk, so this must report
    // dims_b — not dims_a, the stale value a pre-fix
    // `pending_started_dims` would still carry.
    let (dims3, out3) = filter.feed(b"\x07tail", dims_c);
    assert_eq!(
        dims3, dims_b,
        "the retained tail from read 2's partial drain must be \
         attributed to dims_b (the read that produced it), not \
         dims_a (a stale earlier run)"
    );
    assert_eq!(out3, b"\x1b]777;emterm;fold;start;2\x07tail");
}

/// AC-10 companion: the overwhelmingly common case — `pending` is EMPTY
/// when a chunk arrives and stays empty after it (no unterminated
/// introducer at all) — attributes directly to `current_dims`, exactly
/// as before this fix. Pins the "no divergence in the common path"
/// half of the contract.
#[test]
fn feed_attributes_a_fully_self_contained_chunk_to_current_dims() {
    let dims = (100u16, 30u16);
    let mut filter = ScrollbackWriteFilter::new();
    let (attributed, out) = filter.feed(b"plain output, no ESC at all", dims);
    assert_eq!(attributed, dims);
    assert_eq!(out, b"plain output, no ESC at all");
    assert_eq!(filter.pending_len(), 0);
}

/// Plain text that contains no ESC at all is emitted directly with no
/// pending held over — the common hot path stays boundary-clean.
#[test]
fn scrollback_filter_plain_text_emits_immediately() {
    let scrollback = new_scrollback(2048);
    let chunk = b"$ echo hello world\r\nhello world\r\n$ ";
    let filter = feed_all(&scrollback, &[chunk]);
    assert_eq!(filter.pending_len(), 0);
    assert_eq!(scrollback.lock().unwrap().read_all(), &chunk[..]);
}

/// A non-strip-target CSI sequence (e.g. SGR color) does NOT force a
/// pending hold — only OSC / APC / DCS introducers do. This keeps the
/// per-chunk emission latency low on typical shell output.
#[test]
fn scrollback_filter_csi_does_not_force_pending() {
    let scrollback = new_scrollback(2048);
    let chunk = b"hi \x1b[31mred\x1b[0m done";
    let filter = feed_all(&scrollback, &[chunk]);
    assert_eq!(filter.pending_len(), 0);
    assert_eq!(scrollback.lock().unwrap().read_all(), &chunk[..]);
}

// ── mux-suppressed-output-fixes task0002: boundary rules aligned with
//    term_core (AC-1 through AC-6) ──────────────────────────────────────

/// AC-1 (FR1; TS-1): a main-buffer OSC whose body ends in ESC at the end of
/// a feed is INCOMPLETE (not aborted — the next byte hasn't arrived yet) and
/// is held whole in `pending`. When the next feed supplies the backslash,
/// the complete OSC is released in one flush and the ring holds it unsplit.
#[test]
fn write_filter_holds_string_ending_in_trailing_esc_until_completed() {
    let mut filter = ScrollbackWriteFilter::new();
    let (_dims, out1) = filter.feed(b"\x1b]11;?\x1b", (80, 24));
    assert!(
        out1.is_empty(),
        "an OSC body ending in ESC is incomplete (not aborted) — held whole"
    );
    assert_eq!(filter.pending(), b"\x1b]11;?\x1b".as_slice());

    let (_dims, out2) = filter.feed(b"\\", (80, 24));
    assert_eq!(
        out2,
        b"\x1b]11;?\x1b\\".to_vec(),
        "the complete OSC is released in one flush, unsplit"
    );
    assert_eq!(filter.pending_len(), 0);
}

/// AC-2 (FR5): OSC, DCS and APC bodies followed by ESC plus a non-backslash
/// byte count as CLOSED (aborted) at that ESC — nothing after them is held
/// because of them. A string that begins at that very ESC and is still
/// incomplete at the end of the feed is held from its own start. Here an
/// ESC-aborted OSC is immediately followed by an ESC-aborted DCS (the
/// aborting ESC of each is the introducer of the next), and finally an
/// unterminated APC that stays incomplete to the end.
#[test]
fn write_filter_closes_esc_aborted_strings_and_holds_only_incomplete_tail() {
    let mut filter = ScrollbackWriteFilter::new();
    let mut chunk = Vec::new();
    chunk.extend_from_slice(b"\x1b]0;osc"); // OSC introducer + body
    chunk.extend_from_slice(b"\x1b"); // aborts the OSC
    chunk.extend_from_slice(b"Pdcs"); // DCS introducer (the aborting ESC's 'P') + body
    chunk.extend_from_slice(b"\x1b"); // aborts the DCS
    chunk.extend_from_slice(b"_apcbody"); // APC introducer (the aborting ESC's '_') + body, no terminator

    let (_dims, out) = filter.feed(&chunk, (80, 24));
    assert_eq!(
        out,
        b"\x1b]0;osc\x1bPdcs".to_vec(),
        "the aborted OSC and the aborted DCS are both closed and flushed; \
         nothing after them is held because of them"
    );
    assert_eq!(
        filter.pending(),
        b"\x1b_apcbody".as_slice(),
        "the still-open APC is held from its own start"
    );
}

/// AC-3 (FR5): `ESC ESC` followed by an OSC introducer treats the SECOND ESC
/// as the introducer — the first ESC is superseded (flushed as an ordinary
/// byte, not held, and not treated as a fresh OSC/DCS/APC attempt itself),
/// and the OSC the second ESC introduces is held whole while it stays
/// incomplete. A byte-blind scanner that just skips 2 bytes past any
/// non-`_`/`P`/`]` byte (never re-examining the second ESC) would instead
/// treat the whole thing as ordinary text and never hold anything for the
/// still-open OSC — this input has NO terminator, so that difference is
/// observable.
#[test]
fn write_filter_double_esc_treats_the_second_esc_as_the_introducer() {
    let mut filter = ScrollbackWriteFilter::new();
    let (_dims, out1) = filter.feed(b"\x1b\x1b]11;?", (80, 24));
    assert_eq!(
        out1,
        b"\x1b".to_vec(),
        "only the superseded FIRST esc flushes; the OSC the second esc \
         introduces is still incomplete and must be held"
    );
    assert_eq!(
        filter.pending(),
        b"\x1b]11;?".as_slice(),
        "pending must start at the SECOND esc, not the first"
    );

    let (_dims, out2) = filter.feed(b"\x07", (80, 24));
    assert_eq!(out2, b"\x1b]11;?\x07".to_vec());
    assert_eq!(filter.pending_len(), 0);
}

/// AC-3 (FR5): `ESC (` (charset designation) ALWAYS consumes the very next
/// byte as the designator, even when that byte is itself an ESC — so an
/// `ESC ]` right after `ESC (` never introduces an OSC; nothing is held,
/// even when there is no terminator anywhere in the fed bytes (a scanner
/// that (incorrectly) re-examined the designator byte as a fresh introducer
/// would instead hold everything from there to the end, waiting forever for
/// a terminator that was never going to be an OSC's in the first place).
#[test]
fn write_filter_charset_designator_consumes_the_next_byte_even_if_esc() {
    let mut filter = ScrollbackWriteFilter::new();
    // `ESC (` then `ESC ]11;?tail` (no BEL/ST anywhere) — the second ESC is
    // swallowed as the designator for `(`, so what follows (`]11;?tail`) is
    // ordinary bytes, never a recognized (and therefore never a HELD) OSC.
    let chunk = b"\x1b(\x1b]11;?tail".to_vec();
    let (_dims, out) = filter.feed(&chunk, (80, 24));
    assert_eq!(
        out, chunk,
        "the designator-consumed ESC never introduces an OSC; nothing is \
         held even though the presumed OSC body never terminates"
    );
    assert_eq!(filter.pending_len(), 0);
}

/// AC-3 (FR5): `ESC X` and `ESC ^` are complete two-byte sequences, not
/// strings — they never force a pending hold, and text around them flushes
/// straight through.
#[test]
fn write_filter_two_byte_escapes_x_and_caret_do_not_force_a_hold() {
    let mut filter = ScrollbackWriteFilter::new();
    let chunk = b"\x1bXafter1\x1b^after2".to_vec();
    let (_dims, out) = filter.feed(&chunk, (80, 24));
    assert_eq!(out, chunk);
    assert_eq!(filter.pending_len(), 0);
}

/// AC-3 (FR5): a lone trailing ESC (nothing after it in the fed stream) is
/// held — we cannot yet tell whether it introduces a string — and is
/// released once the next feed's leading byte disambiguates it. Here it
/// turns out to be a CSI (`ESC [`), not a strip target, so the whole thing
/// flushes once the disambiguating byte arrives. (An SGR CSI, not a device
/// query — a device query would be REMOVED by the strip step for unrelated
/// reasons (FR3's own concern), which would make a byte-equality assertion
/// here about the wrong thing.)
#[test]
fn write_filter_lone_trailing_esc_is_held_until_the_next_byte_disambiguates_it() {
    let mut filter = ScrollbackWriteFilter::new();
    let (_dims, out1) = filter.feed(b"plain text\x1b", (80, 24));
    assert_eq!(out1, b"plain text".to_vec());
    assert_eq!(filter.pending(), b"\x1b".as_slice());

    let (_dims, out2) = filter.feed(b"[31m", (80, 24));
    assert_eq!(
        out2,
        b"\x1b[31m".to_vec(),
        "the held ESC turned out to introduce a CSI, not a strip target — \
         it releases along with the rest once disambiguated"
    );
    assert_eq!(filter.pending_len(), 0);
}

/// AC-5 (FR5, postcondition): reusing the AC-1 to AC-4 inputs, split at
/// EVERY position, feeding the two halves through a fresh filter across two
/// `feed` calls must flush the same bytes and leave the same `pending` run
/// as feeding the whole input in one shot — the boundary decision never
/// depends on where a read happened to split the stream. Every resulting
/// `pending` also has the shape the postcondition allows: empty, a lone
/// ESC, or a run starting with its own OSC/DCS/APC opening ESC.
///
/// Oracle: for each corpus item taken as a whole, `term_core` (fed the same
/// bytes plus a literal probe marker) is asked whether IT still considers a
/// string open at the end of the item — if the probe is swallowed (never
/// printed to the grid), `term_core` says "still open"; if it prints
/// literally, `term_core` says "closed". This filter's own open/closed
/// verdict (whether `pending` ends up empty) must agree.
///
/// The AC-4 item's CSI is SGR (`ESC[31m`), not a device query: a device
/// query is a strip TARGET removed by `strip_pty_output_for_scrollback_write`
/// (FR3's own, unrelated concern), and whether a strip target gets removed
/// legitimately depends on which single `feed` call sees it whole — CSI
/// content is explicitly out of this filter's hold contract (it is never
/// held, so it can legitimately land in more than one flush), so a query
/// split across the two split-feed calls here would make the split-invariance
/// assertion fail for a reason this task does not own. An SGR CSI is not a
/// strip target either way, so the flushed bytes stay identical regardless
/// of where it is split.
#[test]
fn write_filter_boundaries_are_split_position_independent_and_match_term_core() {
    use term_core::terminal_core::TerminalCore;

    let corpus: Vec<Vec<u8>> = vec![
        b"\x1b]11;?\x1b\\".to_vec(),                      // AC-1, closed
        b"\x1b]11;?\x1b".to_vec(),                        // AC-1, open (trailing ESC in body)
        b"\x1b]0;osc\x1bPdcs\x1b_apcbody\x1b\\".to_vec(), // AC-2, closed
        b"\x1b]0;osc\x1bPdcs\x1b_apcbody".to_vec(),       // AC-2, open (incomplete APC)
        b"\x1b\x1b]11;?\x07".to_vec(),                    // AC-3, double ESC, closed
        b"\x1b(\x1b]11;?\x07tail".to_vec(), // AC-3, charset designator swallow, closed
        b"\x1bXafter1\x1b^after2".to_vec(), // AC-3, two-byte escapes, closed
        b"plain text\x1b".to_vec(),         // AC-3, lone trailing ESC, open
        b"\x1b]0;osc\x1bPdcs\x1b[31mtext".to_vec(), // AC-4, closed (non-query CSI + text, no hold)
    ];

    for input in &corpus {
        let mut whole = ScrollbackWriteFilter::new();
        let (_dims, whole_out) = whole.feed(input, (80, 24));
        let whole_pending = whole.pending().to_vec();

        for split in 0..=input.len() {
            let mut filter = ScrollbackWriteFilter::new();
            let (_dims, out1) = filter.feed(&input[..split], (80, 24));
            let (_dims, out2) = filter.feed(&input[split..], (80, 24));
            let mut combined = out1;
            combined.extend_from_slice(&out2);
            assert_eq!(
                combined, whole_out,
                "split at {split} for {input:?} must flush the same bytes \
                 as an unsplit feed"
            );
            assert_eq!(
                filter.pending(),
                whole_pending.as_slice(),
                "split at {split} for {input:?} must leave the same pending \
                 run as an unsplit feed"
            );

            let pending = filter.pending();
            assert!(
                pending.is_empty()
                    || pending == b"\x1b"
                    || (pending[0] == 0x1b
                        && pending.len() >= 2
                        && matches!(pending[1], b']' | b'P' | b'_')),
                "pending after split {split} for {input:?} has an \
                 unexpected shape: {pending:?}"
            );
        }

        let mut oracle = TerminalCore::new(80, 24, 1000);
        let mut probed = input.clone();
        probed.extend_from_slice(b"PROBEMARK");
        oracle.process_pty_data_fully(&probed);
        let probe_visible = (0..24).any(|r| oracle.get_line_text(r).contains("PROBEMARK"));

        if whole_pending.is_empty() {
            assert!(
                probe_visible,
                "filter says {input:?} is fully closed, but term_core still \
                 swallowed the probe text — the filter closed a string \
                 term_core would keep open"
            );
        } else {
            assert!(
                !probe_visible,
                "filter says {input:?} still has an open string, but \
                 term_core already printed the probe text as literal \
                 characters — the filter held a string term_core would \
                 consider closed"
            );
        }
    }
}

/// AC-6 (TM-2, NFR5): a hostile 64 KiB feed made of nothing but repeated
/// `ESC ]` pairs, each aborting the previous OSC and opening a new one,
/// completes in linear time and holds at most the final (genuinely
/// incomplete) pair.
#[test]
fn write_filter_hostile_aborted_introducers_stream_is_linear_and_holds_only_the_final_pair() {
    let mut filter = ScrollbackWriteFilter::new();
    let pair_count = 32 * 1024; // 64 KiB of `ESC ]` pairs
    let chunk: Vec<u8> = std::iter::repeat_n(*b"\x1b]", pair_count)
        .flatten()
        .collect();

    let start = std::time::Instant::now();
    let (_dims, out) = filter.feed(&chunk, (80, 24));
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "a hostile chain of aborted introducers must scan in linear time; \
         took {elapsed:?}"
    );
    assert_eq!(
        out,
        chunk[..chunk.len() - 2].to_vec(),
        "everything before the final (incomplete) pair is flushed unchanged"
    );
    assert_eq!(
        filter.pending(),
        b"\x1b]".as_slice(),
        "only the final, genuinely incomplete introducer pair is held"
    );
}

/// task0012 AC-5: an `agent-status` OSC report split across chunk
/// boundaries must never land in scrollback. `ScrollbackWriteFilter`
/// already holds ANY unterminated OSC introducer (not just viewer
/// kinds) pending until a terminator arrives — this is a regression /
/// confirmation test that the same guarantee covers `agent-status`
/// reports now that the extraction side is stateful too.
#[test]
fn scrollback_filter_strips_agent_status_report_split_across_reads() {
    let scrollback = new_scrollback(4096);
    let full = b"pre\x1b]777;emterm;agent-status;v=1;state=working;name=claude\x07post".as_slice();
    let split = full
        .windows(4)
        .position(|w| w == b"stat")
        .expect("marker present");
    let (head, tail) = full.split_at(split);
    let filter = feed_all(&scrollback, &[head, tail]);
    assert_eq!(filter.pending_len(), 0);
    assert_eq!(scrollback.lock().unwrap().read_all(), b"prepost");
}

type TestRig = (
    SharedRawPassthrough,
    SharedPassthroughScanner,
    SharedNotificationSender,
    mpsc::Receiver<(PaneId, String)>,
);

fn shared_buffer() -> TestRig {
    let (notif_tx, notif_rx) = mpsc::channel::<(PaneId, String)>(16);
    (
        Arc::new(StdMutex::new(RawPassthroughBuffer::new(
            HIDDEN_PASSTHROUGH_CAPACITY_MUX,
        ))),
        Arc::new(StdMutex::new(PassthroughScanner::new())),
        Arc::new(StdMutex::new(Some(notif_tx))),
        notif_rx,
    )
}

/// TS-19: passthrough sequences feed raw_passthrough while detached.
#[test]
fn capture_passthrough_appends_completed_kitty_apc() {
    let (buf, scanner, notif, _rx) = shared_buffer();
    capture_passthrough(7, b"\x1b_Gi=1;ZZ\x1b\\", &buf, &scanner, &notif);
    let stored = buf.lock().unwrap().read_all();
    assert!(
        stored
            .windows(b"\x1b_Gi=1;ZZ\x1b\\".len())
            .any(|w| w == b"\x1b_Gi=1;ZZ\x1b\\"),
        "captured bytes must contain the original Kitty APC sequence"
    );
}

/// TS-19: a sequence split across two chunks is still recovered because
/// the scanner is stateful and shared.
#[test]
fn capture_passthrough_handles_chunk_boundary() {
    let (buf, scanner, notif, _rx) = shared_buffer();
    capture_passthrough(7, b"\x1b_Gi=1;Z", &buf, &scanner, &notif);
    // Mid-sequence: nothing complete yet.
    assert_eq!(buf.lock().unwrap().len(), 0);
    capture_passthrough(7, b"Z\x1b\\", &buf, &scanner, &notif);
    let stored = buf.lock().unwrap().read_all();
    assert!(
        stored
            .windows(b"\x1b_Gi=1;ZZ\x1b\\".len())
            .any(|w| w == b"\x1b_Gi=1;ZZ\x1b\\"),
        "chunk-split sequence must be reassembled"
    );
}

/// Plain output that contains no image / Markdown OSC must not touch
/// the raw buffer.
#[test]
fn capture_passthrough_ignores_plain_text() {
    let (buf, scanner, notif, _rx) = shared_buffer();
    capture_passthrough(7, b"hello world\n", &buf, &scanner, &notif);
    assert_eq!(buf.lock().unwrap().len(), 0);
}

/// TS-9: a Detached pane emitting `OSC 9 ; msg` forwards a notification
/// through the notification channel and does NOT add it to raw_passthrough.
#[test]
fn capture_passthrough_forwards_osc9_notification() {
    let (buf, scanner, notif, mut rx) = shared_buffer();
    capture_passthrough(7, b"\x1b]9;deploy done\x07", &buf, &scanner, &notif);
    // Notification forwarded.
    let (pane_id, message) = rx.try_recv().expect("notification must be forwarded");
    assert_eq!(pane_id, 7);
    assert_eq!(message, "deploy done");
    // Must NOT be in raw_passthrough (no replay on reattach).
    assert_eq!(
        buf.lock().unwrap().len(),
        0,
        "OSC 9 notification must not enter raw_passthrough"
    );
}

/// FR4: a progress sequence on a Detached pane is not forwarded.
#[test]
fn capture_passthrough_ignores_osc9_progress() {
    let (buf, scanner, notif, mut rx) = shared_buffer();
    capture_passthrough(7, b"\x1b]9;4;1;50\x07", &buf, &scanner, &notif);
    assert!(
        rx.try_recv().is_err(),
        "progress sequence must not forward a notification"
    );
    assert_eq!(buf.lock().unwrap().len(), 0);
}

/// A chunk-split OSC 9 notification is forwarded once the closing chunk
/// arrives, because the scanner is stateful and shared.
#[test]
fn capture_passthrough_forwards_chunk_split_osc9() {
    let (buf, scanner, notif, mut rx) = shared_buffer();
    capture_passthrough(7, b"\x1b]9;long ", &buf, &scanner, &notif);
    assert!(rx.try_recv().is_err(), "no completion yet");
    capture_passthrough(7, b"message\x1b\\", &buf, &scanner, &notif);
    let (_pane_id, message) = rx.try_recv().expect("notification after closing chunk");
    assert_eq!(message, "long message");
}

// ── pty_reader_loop OSC 133 live wiring (task0003, SPEC FR1/FR4/FR5) ──
// End-to-end tests through the ACTUAL live PTY reader path (not just
// `Osc133MarkScanner` in isolation) so the alt-screen gating this
// module wires up is checked against the real code path.

/// AC-5: OSC 133 marks emitted while the pane is on the alternate
/// screen (between `?1049h` and `?1049l`) never reach the
/// agent-status channel — mirroring `term_core`'s own OSC 133
/// alt-screen suppression. Marks emitted on the main screen, in the
/// SAME reader loop run, still do (proving the alt-screen span really
/// was what suppressed the first pair, not something else).
#[test]
fn pty_reader_loop_suppresses_osc133_marks_emitted_on_the_alternate_screen() {
    let (out_tx, _out_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(out_tx)));
    let pane = MuxPane::new_test(1, 80, 24, output_target);

    let (agent_status_tx, mut agent_status_rx) = mpsc::channel::<(PaneId, AgentStatusFeedItem)>(16);
    *pane.agent_status_report_sender.lock().unwrap() = Some(agent_status_tx);

    let mut input = Vec::new();
    // Alt screen: D then A here must be suppressed.
    input.extend_from_slice(b"\x1b[?1049h");
    input.extend_from_slice(b"\x1b]133;D\x07");
    input.extend_from_slice(b"\x1b]133;A\x07");
    input.extend_from_slice(b"\x1b[?1049l");
    // Back on the main screen: D then A here must reach the channel.
    input.extend_from_slice(b"\x1b]133;D\x07");
    input.extend_from_slice(b"\x1b]133;A\x07");

    let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(input));

    pty_reader_loop(
        pane.id,
        reader,
        pane.output_target.clone(),
        pane.shadow_parser.clone(),
        pane.cwd.clone(),
        pane.title.clone(),
        pane.title_sender.clone(),
        pane.notification_sender.clone(),
        pane.agent_status_report_sender.clone(),
        pane.raw_passthrough.clone(),
        pane.passthrough_scanner.clone(),
        pane.scrollback.clone(),
        pane.dims.clone(),
        Arc::new(StdMutex::new(None)),
        pane.output_capture.clone(),
    );

    let mut marks = Vec::new();
    while let Ok((pane_id, item)) = agent_status_rx.try_recv() {
        assert_eq!(pane_id, 1);
        if let AgentStatusFeedItem::Osc133Mark(kind) = item {
            marks.push(kind);
        }
    }
    assert_eq!(
        marks,
        vec![
            crate::prompts::PromptMarkKind::CommandEnd,
            crate::prompts::PromptMarkKind::PromptStart,
        ],
        "only the main-screen D/A pair may reach the channel"
    );
}

/// AC-4 (live-path side): OSC 133 marks are scanned from the LIVE PTY
/// read `pty_reader_loop` is currently processing, never re-derived
/// from this pane's scrollback — a real chunk containing a `Set`-
/// shaped agent-status OSC report next to `D`/`A` marks results in
/// exactly the marks from THIS read reaching the channel, in the same
/// relative order as the report (FR4), regardless of what is already
/// sitting in scrollback from an earlier write.
#[test]
fn pty_reader_loop_feeds_report_then_marks_from_the_same_read_in_order() {
    let (out_tx, _out_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(out_tx)));
    let pane = MuxPane::new_test(2, 80, 24, output_target);
    // Pre-existing scrollback content is irrelevant to what THIS read
    // forwards — it must never be re-scanned as if it were live.
    pane.scrollback
        .lock()
        .unwrap()
        .write(b"\x1b]133;D\x07\x1b]133;A\x07");

    let (agent_status_tx, mut agent_status_rx) = mpsc::channel::<(PaneId, AgentStatusFeedItem)>(16);
    *pane.agent_status_report_sender.lock().unwrap() = Some(agent_status_tx);

    let mut input = Vec::new();
    input.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=working\x07");
    input.extend_from_slice(b"\x1b]133;D\x07");
    input.extend_from_slice(b"\x1b]133;A\x07");
    let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(input));

    pty_reader_loop(
        pane.id,
        reader,
        pane.output_target.clone(),
        pane.shadow_parser.clone(),
        pane.cwd.clone(),
        pane.title.clone(),
        pane.title_sender.clone(),
        pane.notification_sender.clone(),
        pane.agent_status_report_sender.clone(),
        pane.raw_passthrough.clone(),
        pane.passthrough_scanner.clone(),
        pane.scrollback.clone(),
        pane.dims.clone(),
        Arc::new(StdMutex::new(None)),
        pane.output_capture.clone(),
    );

    let mut items = Vec::new();
    while let Ok((pane_id, item)) = agent_status_rx.try_recv() {
        assert_eq!(pane_id, 2);
        items.push(item);
    }
    assert_eq!(items.len(), 3, "one report + two marks from THIS read only");
    assert!(
        matches!(&items[0], AgentStatusFeedItem::Report(r) if r == "emterm;agent-status;v=1;state=working")
    );
    assert!(matches!(
        items[1],
        AgentStatusFeedItem::Osc133Mark(crate::prompts::PromptMarkKind::CommandEnd)
    ));
    assert!(matches!(
        items[2],
        AgentStatusFeedItem::Osc133Mark(crate::prompts::PromptMarkKind::PromptStart)
    ));
}

/// Regression for the FR4 ordering bug (concatenating a full-chunk
/// report scan with a separately-scanned mark list loses TRUE relative
/// order): a single read whose actual byte order is `D` → `A` → `Set`
/// must forward items in exactly that order, NOT `Set, D, A` (which is
/// what naive "reports first, then marks" concatenation would produce
/// even though this read never emitted the report first).
#[test]
fn pty_reader_loop_preserves_true_byte_order_when_marks_precede_a_report_in_the_same_read() {
    let (out_tx, _out_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(out_tx)));
    let pane = MuxPane::new_test(4, 80, 24, output_target);

    let (agent_status_tx, mut agent_status_rx) = mpsc::channel::<(PaneId, AgentStatusFeedItem)>(16);
    *pane.agent_status_report_sender.lock().unwrap() = Some(agent_status_tx);

    let mut input = Vec::new();
    input.extend_from_slice(b"\x1b]133;D\x07");
    input.extend_from_slice(b"\x1b]133;A\x07");
    input.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=working\x07");
    let reader: Box<dyn Read + Send> = Box::new(std::io::Cursor::new(input));

    pty_reader_loop(
        pane.id,
        reader,
        pane.output_target.clone(),
        pane.shadow_parser.clone(),
        pane.cwd.clone(),
        pane.title.clone(),
        pane.title_sender.clone(),
        pane.notification_sender.clone(),
        pane.agent_status_report_sender.clone(),
        pane.raw_passthrough.clone(),
        pane.passthrough_scanner.clone(),
        pane.scrollback.clone(),
        pane.dims.clone(),
        Arc::new(StdMutex::new(None)),
        pane.output_capture.clone(),
    );

    let mut items = Vec::new();
    while let Ok((pane_id, item)) = agent_status_rx.try_recv() {
        assert_eq!(pane_id, 4);
        items.push(item);
    }
    assert_eq!(items.len(), 3, "two marks + one report from THIS read");
    assert!(
        matches!(
            items[0],
            AgentStatusFeedItem::Osc133Mark(crate::prompts::PromptMarkKind::CommandEnd)
        ),
        "D must be forwarded first, matching the true byte order: {items:?}"
    );
    assert!(
        matches!(
            items[1],
            AgentStatusFeedItem::Osc133Mark(crate::prompts::PromptMarkKind::PromptStart)
        ),
        "A must be forwarded second, matching the true byte order: {items:?}"
    );
    assert!(
        matches!(&items[2], AgentStatusFeedItem::Report(r) if r == "emterm;agent-status;v=1;state=working"),
        "the report must be forwarded LAST since it appeared last in the \
         byte stream, not first (the bug this test guards against): \
         {items:?}"
    );
}

// ── forward_agent_status_items (task0012 AC-6 / try_send_drops_reports;
//    task0003 generalized to carry OSC 133 marks alongside reports) ────

/// Baseline: a healthy channel delivers via the `try_send` fast path,
/// no blocking.
#[test]
fn forward_agent_status_items_delivers_via_try_send_when_channel_has_room() {
    let (tx, mut rx) = mpsc::channel::<(PaneId, AgentStatusFeedItem)>(4);
    let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(Some(tx)));
    forward_agent_status_items(
        3,
        vec![AgentStatusFeedItem::Report(
            "emterm;agent-status;clear".to_string(),
        )],
        &sender,
    );
    let (pane_id, item) = rx.try_recv().expect("item must be delivered");
    assert_eq!(pane_id, 3);
    assert!(matches!(item, AgentStatusFeedItem::Report(r) if r == "emterm;agent-status;clear"));
}

/// A live OSC 133 mark travels through the same forwarding path as a
/// report (task0003, SPEC FR4).
#[test]
fn forward_agent_status_items_delivers_osc133_mark() {
    let (tx, mut rx) = mpsc::channel::<(PaneId, AgentStatusFeedItem)>(4);
    let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(Some(tx)));
    forward_agent_status_items(
        3,
        vec![AgentStatusFeedItem::Osc133Mark(
            crate::prompts::PromptMarkKind::CommandEnd,
        )],
        &sender,
    );
    let (pane_id, item) = rx.try_recv().expect("item must be delivered");
    assert_eq!(pane_id, 3);
    assert!(matches!(
        item,
        AgentStatusFeedItem::Osc133Mark(crate::prompts::PromptMarkKind::CommandEnd)
    ));
}

/// Empty item list is a no-op — no send attempted, no panic on a
/// `None` sender either.
#[test]
fn forward_agent_status_items_empty_list_is_noop() {
    let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(None));
    // Must not panic even though the sender is unset.
    forward_agent_status_items(1, Vec::new(), &sender);
}

/// AC-6: a full channel must NOT silently drop an accepted item
/// (review round-1 stable_id `try_send_drops_reports`). This proves the
/// blocking-send fallback actually delivers once capacity frees, rather
/// than the old best-effort `try_send` that discarded on `Full`.
#[test]
fn forward_agent_status_items_blocks_instead_of_dropping_on_full_channel() {
    let (tx, mut rx) = mpsc::channel::<(PaneId, AgentStatusFeedItem)>(1);
    // Fill the one available slot directly so the channel is Full.
    tx.try_send((1, AgentStatusFeedItem::Report("first".to_string())))
        .unwrap();
    let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(Some(tx)));

    let sender_for_thread = sender.clone();
    let handle = std::thread::spawn(move || {
        forward_agent_status_items(
            1,
            vec![AgentStatusFeedItem::Report("second".to_string())],
            &sender_for_thread,
        );
    });

    // Drain the first item, freeing the slot the blocked send is
    // waiting on.
    let (first_pane, first_item) = rx.blocking_recv().expect("first item must still arrive");
    assert_eq!(first_pane, 1);
    assert!(matches!(first_item, AgentStatusFeedItem::Report(r) if r == "first"));

    // The blocking send only completes once the slot is free, so this
    // recv is what proves "second" was not dropped.
    let (second_pane, second_item) = rx.blocking_recv().expect("second item must not be dropped");
    assert_eq!(second_pane, 1);
    assert!(matches!(second_item, AgentStatusFeedItem::Report(r) if r == "second"));

    handle.join().expect("forwarding thread must not panic");
}

/// A closed channel (receiver dropped) is handled gracefully — the send
/// is a no-op, no panic, no hang.
#[test]
fn forward_agent_status_items_closed_channel_does_not_panic() {
    let (tx, rx) = mpsc::channel::<(PaneId, AgentStatusFeedItem)>(1);
    drop(rx);
    let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(Some(tx)));
    forward_agent_status_items(
        1,
        vec![AgentStatusFeedItem::Report("orphaned".to_string())],
        &sender,
    );
}

// ── mux-snapshot-output-boundary task0001: output sequence capture,
//    sender-bound suppression, ordered delivery (AC-1 through AC-6) ─────

/// Test-only `Read` that hands out each of `chunks`, in order, one per
/// `read()` call (Test Notes: "drive the reader through `pty_reader_loop`
/// with a test-controlled reader that hands out scripted chunks on
/// demand"). Once exhausted, reports EOF (`Ok(0)`) — the reader's normal
/// termination path.
struct ScriptedReader {
    chunks: std::collections::VecDeque<Vec<u8>>,
}

impl ScriptedReader {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: chunks.into_iter().collect(),
        }
    }
}

impl Read for ScriptedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.chunks.pop_front() {
            Some(chunk) => {
                let n = chunk.len();
                buf[..n].copy_from_slice(&chunk);
                Ok(n)
            }
            None => Ok(0),
        }
    }
}

/// Spawn `pty_reader_loop` on a background thread against `pane`, fed
/// `chunks` by a [`ScriptedReader`], with a generously-sized capacity-16
/// output channel (backpressure is AC-3's exclusive concern; every other
/// AC in this section uses a channel that never fills). Returns the join
/// handle, the sender (so a test can also enqueue a stand-in snapshot on
/// the exact same channel and record a boundary against it), and the
/// output receiver.
fn spawn_reader_with_chunks(
    pane: &MuxPane,
    chunks: Vec<Vec<u8>>,
) -> (
    std::thread::JoinHandle<()>,
    mpsc::Sender<PtyOutputChunk>,
    mpsc::Receiver<PtyOutputChunk>,
) {
    let (tx, rx) = mpsc::channel::<PtyOutputChunk>(16);
    *pane.output_target.lock().unwrap() = PaneOutputTarget::Connected(tx.clone());
    let handle = std::thread::spawn({
        let output_target = pane.output_target.clone();
        let shadow_parser = pane.shadow_parser.clone();
        let cwd = pane.cwd.clone();
        let title = pane.title.clone();
        let title_sender = pane.title_sender.clone();
        let notification_sender = pane.notification_sender.clone();
        let agent_status_report_sender = pane.agent_status_report_sender.clone();
        let raw_passthrough = pane.raw_passthrough.clone();
        let passthrough_scanner = pane.passthrough_scanner.clone();
        let scrollback = pane.scrollback.clone();
        let dims = pane.dims.clone();
        let output_capture = pane.output_capture.clone();
        let pane_id = pane.id;
        move || {
            pty_reader_loop(
                pane_id,
                Box::new(ScriptedReader::new(chunks)),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                output_capture,
            );
        }
    });
    (handle, tx, rx)
}

/// AC-2 (FR1, FR2; TS-4): numbers start at 1 and increase by exactly one
/// per non-empty read, including a chunk that is entirely alt-screen (and
/// so writes nothing to the ring) — the number tracks "a read happened",
/// not "the ring changed". EOF takes no number (the loop exits right
/// after the last non-empty read's number is committed, with no further
/// `capture()` call).
#[test]
fn output_sequence_numbers_increment_once_per_non_empty_read_including_alt_screen_only_chunks() {
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        mpsc::channel(16).0,
    )));
    let pane = MuxPane::new_test(1, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();

    // Chunk 1: plain main-buffer text.
    let chunk1 = b"hello".to_vec();
    // Chunk 2: enters the alternate screen and writes a full-screen TUI
    // frame there, then leaves it again in the SAME read — the scan
    // exits the same buffer it entered (main), so the write path's
    // conservative fallback does not apply; nothing from this chunk's alt
    // span reaches the ring, yet it must still get its own number.
    let mut chunk2 = Vec::new();
    chunk2.extend_from_slice(b"\x1b[?1049h");
    chunk2.extend_from_slice(b"tui frame content");
    chunk2.extend_from_slice(b"\x1b[?1049l");

    let (handle, _tx, mut rx) =
        spawn_reader_with_chunks(&pane, vec![chunk1.clone(), chunk2.clone()]);

    // Drain to EOF (capacity-16 channel never fills for two small chunks
    // plus the empty EOF signal).
    handle.join().unwrap();
    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }
    assert_eq!(received.len(), 3, "chunk1, chunk2, and the empty EOF chunk");
    assert_eq!(received[0].data, chunk1);
    assert_eq!(received[1].data, chunk2);
    assert!(received[2].data.is_empty(), "EOF chunk carries empty data");

    let (_, last_number) = output_capture.captured_read(|| ());
    assert_eq!(
        last_number, 2,
        "two non-empty reads (including the alt-screen-only one) must \
         each get exactly one number; EOF must not bump it further"
    );

    let (ring_bytes, _segments) = pane.scrollback.lock().unwrap().read_segments();
    assert!(
        !String::from_utf8_lossy(&ring_bytes).contains("tui frame content"),
        "chunk 2's alt-screen content must never reach the ring, even \
         though it was still assigned a number"
    );
}

/// AC-2 ("Snapshot during capture"): with the reader paused at P1 (after
/// the shadow update, before the ring write — still inside the SAME
/// `capture()` step, so `last_captured`'s exclusion is held throughout),
/// a concurrent `captured_read` (standing in for any snapshot path's own
/// read of ring + shadow + number) must wait for the capture step to
/// finish. Once released, the captured ring, shadow dump and
/// last-captured number all reflect the SAME chunk.
#[test]
fn snapshot_style_captured_read_waits_for_the_capture_step_to_finish() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        mpsc::channel(16).0,
    )));
    let pane = MuxPane::new_test(2, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();

    let (arrived_rx, release_tx) = output_capture.p1.arm();
    let (handle, _tx, mut rx) =
        spawn_reader_with_chunks(&pane, vec![b"insert-line-chunk".to_vec()]);

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P1");

    let snapshot_done = Arc::new(AtomicBool::new(false));
    let sd = snapshot_done.clone();
    let oc = output_capture.clone();
    let scrollback = pane.scrollback.clone();
    let shadow_parser = pane.shadow_parser.clone();
    let snapshot_thread = std::thread::spawn(move || {
        let ((ring, shadow_size), number) = oc.captured_read(|| {
            let (bytes, _segments) = scrollback.lock().unwrap().read_segments();
            let size = shadow_parser.lock().unwrap().screen().size();
            (bytes, size)
        });
        sd.store(true, Ordering::SeqCst);
        (ring, shadow_size, number)
    });

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        !snapshot_done.load(Ordering::SeqCst),
        "a concurrent captured_read must wait for the paused capture step \
         to finish"
    );

    release_tx.send(()).unwrap();
    let (ring, _shadow_size, number) = snapshot_thread.join().unwrap();
    assert_eq!(
        number, 1,
        "the captured number must reflect the just-finished chunk"
    );
    assert!(
        String::from_utf8_lossy(&ring).contains("insert-line-chunk"),
        "the captured ring must already contain the just-finished chunk's \
         bytes — capture() commits the ring write and the number together"
    );

    handle.join().unwrap();
    while rx.try_recv().is_ok() {}
}

/// AC-3 (FR4, FR11; TS-5), "Covered chunk": the reader is held at P3 with
/// a full capacity-1 channel. While it waits, a stand-in "on-demand
/// snapshot" enters the SAME channel and records the boundary covering
/// the waiting chunk's number. Once a slot frees up, the reader's
/// re-check finds the chunk covered — the suppression pipeline runs
/// instead of a raw delivery, and since this chunk carries no query and
/// no tail, that pipeline sends NOTHING for it (TM-2). The destination
/// therefore never sees the covered chunk's bytes at all, only the
/// snapshot and then EOF.
#[test]
fn covered_chunk_waiting_for_a_slot_never_reaches_the_destination_after_the_snapshot() {
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));
    let pane = MuxPane::new_test(3, 80, 24, output_target.clone());
    let output_capture = pane.output_capture.clone();

    // Saturate the capacity-1 channel so the reader's first try_send hits
    // Full.
    tx.try_send(PtyOutputChunk::pty_output(3, b"filler".to_vec()))
        .expect("filler must fit in the fresh capacity-1 channel");

    let chunk = b"plain output text".to_vec();
    let (arrived_rx, release_tx) = output_capture.p3.arm();

    let handle = std::thread::spawn({
        let output_target = output_target.clone();
        let shadow_parser = pane.shadow_parser.clone();
        let cwd = pane.cwd.clone();
        let title = pane.title.clone();
        let title_sender = pane.title_sender.clone();
        let notification_sender = pane.notification_sender.clone();
        let agent_status_report_sender = pane.agent_status_report_sender.clone();
        let raw_passthrough = pane.raw_passthrough.clone();
        let passthrough_scanner = pane.passthrough_scanner.clone();
        let scrollback = pane.scrollback.clone();
        let dims = pane.dims.clone();
        let oc = output_capture.clone();
        move || {
            pty_reader_loop(
                3,
                Box::new(ScriptedReader::new(vec![chunk])),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                oc,
            );
        }
    });

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P3 (Full insertion attempt)");

    // Drain the filler, freeing the one slot, then let "the snapshot"
    // occupy it and record the boundary that covers chunk #1 — mirroring
    // an on-demand snapshot entering the channel first and recording (S,
    // B) together with its own insertion.
    let filler = rx.try_recv().expect("filler must be present");
    assert_eq!(filler.data, b"filler");
    tx.try_send(PtyOutputChunk::pty_output(3, b"SNAPSHOT".to_vec()))
        .expect("the stand-in snapshot must fit in the freed slot");
    output_capture.record_boundary(&tx, 1);

    release_tx.send(()).unwrap();

    // The reader is now blocked in reserve_owned() until this slot frees.
    let snapshot = rx.blocking_recv().expect("the snapshot must be delivered");
    assert_eq!(snapshot.data, b"SNAPSHOT");

    let eof = rx.blocking_recv().expect("EOF chunk must follow");
    assert!(
        eof.data.is_empty(),
        "the covered chunk carried no query and no tail, so the \
         suppression pipeline sends nothing for it — the very next item \
         after the snapshot must be EOF, never \"plain output text\""
    );
    assert!(
        rx.try_recv().is_err(),
        "nothing else must have been delivered"
    );

    handle.join().unwrap();
}

/// AC-3, "Closed destination": while the reader waits on `reserve_owned`
/// after a Full non-blocking attempt, the destination closes. The reader
/// must run `capture_passthrough` exactly once and, since `output_target`
/// still points at the same sender it was waiting on, switch it to
/// `Detached(NetworkDetach)`.
#[test]
fn closed_destination_while_reader_waits_for_a_slot_runs_passthrough_once_and_detaches() {
    let (tx, rx) = mpsc::channel::<PtyOutputChunk>(1);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));
    let pane = MuxPane::new_test(4, 80, 24, output_target.clone());
    let output_capture = pane.output_capture.clone();

    tx.try_send(PtyOutputChunk::pty_output(4, b"filler".to_vec()))
        .expect("filler must fit");

    // A complete OSC 9 notification, so a single `capture_passthrough`
    // run is independently observable via the notification channel.
    let mut chunk = Vec::new();
    chunk.extend_from_slice(b"\x1b]9;closed-dest-notice\x07");
    let (notif_tx, mut notif_rx) = mpsc::channel(4);
    *pane.notification_sender.lock().unwrap() = Some(notif_tx);

    let (arrived_rx, release_tx) = output_capture.p3.arm();

    let handle = std::thread::spawn({
        let output_target = output_target.clone();
        let shadow_parser = pane.shadow_parser.clone();
        let cwd = pane.cwd.clone();
        let title = pane.title.clone();
        let title_sender = pane.title_sender.clone();
        let notification_sender = pane.notification_sender.clone();
        let agent_status_report_sender = pane.agent_status_report_sender.clone();
        let raw_passthrough = pane.raw_passthrough.clone();
        let passthrough_scanner = pane.passthrough_scanner.clone();
        let scrollback = pane.scrollback.clone();
        let dims = pane.dims.clone();
        let oc = output_capture.clone();
        move || {
            pty_reader_loop(
                4,
                Box::new(ScriptedReader::new(vec![chunk])),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                oc,
            );
        }
    });

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P3");

    // Close the destination the reader is about to wait on.
    drop(rx);
    release_tx.send(()).unwrap();

    handle.join().unwrap();

    let mut notifications = Vec::new();
    while let Ok((pane_id, msg)) = notif_rx.try_recv() {
        assert_eq!(pane_id, 4);
        notifications.push(msg);
    }
    assert_eq!(
        notifications.len(),
        1,
        "capture_passthrough must run exactly once for the closed-while-\
         waiting chunk"
    );

    assert!(
        matches!(
            &*output_target.lock().unwrap(),
            PaneOutputTarget::Detached {
                reason: DetachReason::NetworkDetach,
                ..
            }
        ),
        "the target still pointed at the closed sender, so it must switch \
         to Detached(NetworkDetach)"
    );
}

/// AC-3, "Closed destination" (D4 nuance): if a NEWER owner has already
/// swapped `output_target` to a different, still-open destination by the
/// time the reserved slot on the OLD (now closed) destination fails, the
/// reader must NOT overwrite that newer target — "switch to Detached
/// only if the target still points at S".
#[test]
fn closed_destination_does_not_override_a_newer_owners_target() {
    let (tx_a, rx_a) = mpsc::channel::<PtyOutputChunk>(1);
    let (tx_b, _rx_b) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx_a.clone())));
    let pane = MuxPane::new_test(5, 80, 24, output_target.clone());
    let output_capture = pane.output_capture.clone();

    tx_a.try_send(PtyOutputChunk::pty_output(5, b"filler".to_vec()))
        .expect("filler must fit");

    let (arrived_rx, release_tx) = output_capture.p3.arm();

    let handle = std::thread::spawn({
        let output_target = output_target.clone();
        let shadow_parser = pane.shadow_parser.clone();
        let cwd = pane.cwd.clone();
        let title = pane.title.clone();
        let title_sender = pane.title_sender.clone();
        let notification_sender = pane.notification_sender.clone();
        let agent_status_report_sender = pane.agent_status_report_sender.clone();
        let raw_passthrough = pane.raw_passthrough.clone();
        let passthrough_scanner = pane.passthrough_scanner.clone();
        let scrollback = pane.scrollback.clone();
        let dims = pane.dims.clone();
        let oc = output_capture.clone();
        move || {
            pty_reader_loop(
                5,
                Box::new(ScriptedReader::new(vec![b"plain".to_vec()])),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                oc,
            );
        }
    });

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P3");

    // A newer owner takes the pane over onto a different, open channel...
    *output_target.lock().unwrap() = PaneOutputTarget::Connected(tx_b.clone());
    // ...and the OLD destination this reader is still waiting on closes.
    drop(rx_a);

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    assert!(
        matches!(
            &*output_target.lock().unwrap(),
            PaneOutputTarget::Connected(cur) if cur.same_channel(&tx_b)
        ),
        "a newer owner's Connected target must be left intact, not \
         overwritten to Detached, when the reader's OLD destination \
         closes"
    );
}

/// AC-1 (FR1, FR2, FR3; TS-1..3), on-demand-snapshot path, combined with
/// AC-6's device-query timing (FR9; TS-9): the reader is paused at P2 —
/// capture step finished, `output_target` not yet taken — on a chunk that
/// contains an insert-line sequence (`CSI L`) and a device query
/// (`CSI 6n`). While paused, a stand-in on-demand snapshot is assembled
/// with the EXACT primitives `handle_request_pane_snapshot` uses
/// (`OutputCapture::captured_read` for the ring+shadow read,
/// `build_snapshot_bytes_for_ring` for assembly, `encode_snapshot_segments`
/// for the wire encoding) and delivered to the pane's own channel, then
/// the boundary is recorded for it — mirroring `enqueue_pane_output_chunk`
/// committing (S, B) together with the insertion.
///
/// Once released, the reader's own forward decision finds the chunk
/// covered: the destination receives the snapshot and NEVER the paused
/// chunk's raw bytes after it. The chunk's embedded device query is
/// re-delivered via the FR9 replacement, in order, after the snapshot and
/// before the next (unsuppressed) reader chunk. Feeding [snapshot bytes,
/// then every later delivered byte] into a fresh `term_core` gives the
/// same screen, cursor and scroll region as feeding the whole raw stream
/// into a fresh reference once.
#[test]
fn on_demand_snapshot_shaped_suppression_matches_a_reference_and_redelivers_the_query() {
    let cols: u16 = 80;
    let rows: u16 = 24;

    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        mpsc::channel(16).0,
    )));
    let pane = MuxPane::new_test(7, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    // Seed a prior (unsuppressed) read establishing baseline screen
    // content, without driving it through the reader — this only needs
    // to bump the capture step's number to 1 and leave the ring/shadow in
    // a realistic pre-state, mirroring "a read already happened before
    // the one this test pauses".
    let baseline: &[u8] = b"line1\r\nline2\r\nline3\r\n";
    output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(baseline);
        pane.scrollback.lock().unwrap().write(baseline);
    });

    // The paused chunk: move to top-left, insert a line (CSI L — the
    // "insert-line sequence" AC-1's setup names), then a device query.
    let paused_chunk = b"\x1b[1;1H\x1b[L\x1b[6n".to_vec();
    // A later, unsuppressed chunk delivered normally after resume.
    let continuation_chunk = b"more output after resume\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (handle, tx, mut rx) = spawn_reader_with_chunks(
        &pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    );

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the paused chunk");

    // Assemble the stand-in on-demand snapshot with the SAME primitives
    // handle_request_pane_snapshot uses.
    let (
        (scrollback_data, scrollback_segments, ring_wrapped, screen_data, alt_screen, current_dims),
        boundary,
    ) = output_capture.captured_read(|| {
        let (scrollback_data, scrollback_segments, ring_wrapped) = pane
            .scrollback
            .lock()
            .unwrap()
            .read_segments_with_wrap_state();
        let (screen_data, alt_screen, current_dims) = {
            let parser = pane.shadow_parser.lock().unwrap();
            let screen = parser.screen();
            let (rows, cols) = screen.size();
            (
                screen.contents_formatted(),
                screen.alternate_screen(),
                (cols, rows),
            )
        };
        (
            scrollback_data,
            scrollback_segments,
            ring_wrapped,
            screen_data,
            alt_screen,
            current_dims,
        )
    });
    assert_eq!(
        boundary, 2,
        "the paused chunk (the second capture() call) must already be \
         reflected by this captured_read, taken while paused at P2"
    );
    let (snapshot_payload, snapshot_segments) =
        crate::mux::snapshot_bytes::build_snapshot_bytes_for_ring(
            &scrollback_data,
            &scrollback_segments,
            &screen_data,
            alt_screen,
            ring_wrapped,
            current_dims,
            10_000,
        );
    let encoded_snapshot =
        crate::mux::session::pane::encode_snapshot_segments(&snapshot_payload, &snapshot_segments);

    tx.try_send(PtyOutputChunk::snapshot(pane.id, encoded_snapshot.clone()))
        .expect("the stand-in snapshot must fit in the fresh capacity-16 channel");
    output_capture.record_boundary(&tx, boundary);

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }

    assert!(
        !received.iter().any(|c| c.data == paused_chunk),
        "the paused chunk's raw bytes must never reach the destination \
         after the snapshot"
    );

    assert_eq!(
        received[0].data, encoded_snapshot,
        "the destination must receive the snapshot first"
    );
    assert_eq!(
        received[0].kind,
        crate::mux::session::pane::ChunkKind::Snapshot
    );

    assert_eq!(
        received[1].data, b"\x1b[6n",
        "the paused chunk's device query must be re-delivered via the \
         FR9 replacement, immediately after the snapshot and before the \
         next reader chunk"
    );
    assert_eq!(
        received[1].kind,
        crate::mux::session::pane::ChunkKind::PtyOutput
    );

    assert_eq!(
        received[2].data, continuation_chunk,
        "the next (unsuppressed) reader chunk must follow the replacement"
    );

    assert!(received[3].data.is_empty(), "EOF chunk must follow");
    assert_eq!(received.len(), 4, "nothing else must have been delivered");

    // Reference equality: feed a fresh term_core [snapshot replay, then
    // every later delivered byte] and compare against a fresh reference
    // fed the whole raw stream once.
    use term_core::terminal_core::{MODE_ORIGIN, TerminalCore};
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&snapshot_payload, &to_replay_segments(&snapshot_segments));
    client.process_pty_data_fully(b"\x1b[6n");
    client.process_pty_data_fully(&continuation_chunk);

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(baseline);
    reference.process_pty_data_fully(&paused_chunk);
    reference.process_pty_data_fully(&continuation_chunk);

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
    assert_eq!(
        client.get_scroll_region_top(),
        reference.get_scroll_region_top()
    );
    assert_eq!(
        client.get_scroll_region_bottom(),
        reference.get_scroll_region_bottom()
    );
    assert_eq!(
        client.get_mode(MODE_ORIGIN),
        reference.get_mode(MODE_ORIGIN)
    );
    assert_eq!(client.get_cursor_row(), reference.get_cursor_row());
    assert_eq!(client.get_cursor_col(), reference.get_cursor_col());
}

/// AC-5 (FR6, FR8; TS-7), "OSC 9": a complete OSC 9 in a suppressed chunk
/// produces exactly one notification, and (since the OSC 9 body is
/// neither a query nor an incomplete tail) the FR9/FR10 replacement is
/// empty — TM-2 forbids sending an empty chunk, so the destination never
/// receives this chunk's bytes at all.
#[test]
fn osc9_notification_in_a_suppressed_chunk_fires_exactly_once_and_the_chunk_is_withheld() {
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        mpsc::channel(16).0,
    )));
    let pane = MuxPane::new_test(8, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();

    let (notif_tx, mut notif_rx) = mpsc::channel(4);
    *pane.notification_sender.lock().unwrap() = Some(notif_tx);

    // Baseline capture (number 1) so the chunk under test is number 2 —
    // matches this section's convention elsewhere and keeps the boundary
    // math obvious.
    output_capture.capture(|| ());

    let chunk = b"\x1b]9;suppressed-notice\x07".to_vec();
    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (handle, tx, mut rx) = spawn_reader_with_chunks(&pane, vec![chunk.clone()]);

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");
    output_capture.record_boundary(&tx, 2);
    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut notifications = Vec::new();
    while let Ok((pane_id, msg)) = notif_rx.try_recv() {
        assert_eq!(pane_id, 8);
        notifications.push(msg);
    }
    assert_eq!(
        notifications.len(),
        1,
        "a complete OSC 9 in a suppressed chunk must produce exactly one \
         notification"
    );

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }
    assert!(
        !received.iter().any(|c| c.data == chunk),
        "the destination must never receive the suppressed chunk's bytes"
    );
    assert_eq!(received.len(), 1, "only the EOF chunk must follow");
    assert!(received[0].data.is_empty());
}

/// AC-5, "Split OSC 9": an OSC 9 split across two consecutive SUPPRESSED
/// chunks must not fire more than once (D6: `discard_partial` wipes the
/// SERVER's own passthrough-scanner in-progress state after each
/// suppressed chunk, so the second half is scanned fresh and cannot
/// complete a notification the first half started — "at most once"
/// permits firing zero times here, which is what this produces; it must
/// never fire twice or hang). This is independent of the write filter's
/// OWN OSC tracking (D8's pending-tail mechanism), which legitimately
/// reproduces chunk_a's bytes to the CLIENT via the FR9/FR10 replacement
/// — a different, correct mechanism this test also documents.
#[test]
fn split_osc9_across_two_suppressed_chunks_never_fires_more_than_once() {
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        mpsc::channel(16).0,
    )));
    let pane = MuxPane::new_test(9, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();

    let (notif_tx, mut notif_rx) = mpsc::channel(4);
    *pane.notification_sender.lock().unwrap() = Some(notif_tx);

    output_capture.capture(|| ()); // baseline, number 1

    let chunk_a = b"\x1b]9;spl".to_vec();
    let chunk_b = b"it-notice\x07".to_vec();

    // Pause on chunk_a via P2, record its boundary so it is suppressed,
    // then repeat for chunk_b — both suppressed, matching the scenario
    // this test targets.
    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (handle, tx, mut rx) =
        spawn_reader_with_chunks(&pane, vec![chunk_a.clone(), chunk_b.clone()]);

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on chunk_a");
    output_capture.record_boundary(&tx, 2);

    // FR11: arm the P2 pause for chunk_b BEFORE releasing chunk_a. P2
    // disarms itself after one hit (per PauseHook::hit doc), so it must be
    // re-armed for chunk_b — but arming it only after the release let the
    // reader race ahead and blow past P2 on chunk_b before this test ever
    // re-armed it, occasionally failing the 5s recv_timeout below. Arming
    // while the reader is still paused on chunk_a is safe: `PauseHook::hit`
    // takes its armed state (`Option::take`) before it blocks on release,
    // so re-arming here only affects the NEXT hit.
    let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();
    release_tx.send(()).unwrap();

    arrived_rx_2
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on chunk_b");
    output_capture.record_boundary(&tx, 3);
    release_tx_2.send(()).unwrap();

    handle.join().unwrap();

    let mut notifications = Vec::new();
    while let Ok((_, msg)) = notif_rx.try_recv() {
        notifications.push(msg);
    }
    assert!(
        notifications.len() <= 1,
        "a split OSC 9 across two suppressed chunks must never fire more \
         than once; got {notifications:?}"
    );
    assert_eq!(
        notifications.len(),
        0,
        "discard_partial resets the passthrough scanner right after \
         chunk_a is suppressed, so chunk_b's completion bytes can never \
         complete the notification chunk_a's half started"
    );

    // chunk_a is the ENTIRE unterminated OSC 9 introducer (no BEL/ST
    // inside it), so the scrollback write filter's `pending` holds it
    // verbatim after chunk_a. D8's T mechanism legitimately reproduces
    // that whole pending run as chunk_a's FR9/FR10 replacement — this is
    // NOT the server's own passthrough_scanner (already reset by
    // `discard_partial` above); it is what lets a client-side parser see
    // the same prefix bytes a later, unsuppressed completion would need.
    // chunk_b completes the OSC inside the write filter's own pending
    // tracking and has no query and no incomplete tail of its own, so its
    // replacement is empty — nothing is sent for it beyond EOF.
    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }
    let received_data: Vec<&[u8]> = received.iter().map(|c| c.data.as_slice()).collect();
    assert_eq!(
        received.len(),
        2,
        "chunk_a's pending-tail replacement, then EOF: {received_data:?}"
    );
    assert_eq!(
        received[0].data, chunk_a,
        "chunk_a is reproduced verbatim via the D8 pending-tail mechanism"
    );
    assert!(received[1].data.is_empty(), "EOF chunk must follow");
}

/// mux-snapshot-output-boundary task0003 (TS-7): AC-5/AC-6 — an OSC 9
/// split across a PRODUCTION on-demand snapshot boundary (not the
/// test-only stand-in `split_osc9_across_two_suppressed_chunks_never_fires_more_than_once`
/// above uses). The introducer + partial body land in the chunk the
/// reader is paused on at P2; the PRODUCTION
/// `handle_request_pane_snapshot` delivers the snapshot to S while
/// paused, covering exactly that chunk; the completing chunk (numbered
/// above the recorded boundary) is delivered unsuppressed.
///
/// AC-5: the daemon's notification channel receives at most one
/// notification for the split OSC 9; the bytes delivered to S after the
/// snapshot contain the OSC 9 exactly once as a complete sequence when
/// concatenated; the total of daemon-side notifications plus complete
/// OSC 9 sequences delivered to S is exactly one.
///
/// AC-6: after that sequence, the pane is switched to `Detached` and fed
/// a bare terminator with no introducer (fires nothing), then a
/// SEPARATE complete OSC 9 (fires exactly once, with its own body).
#[tokio::test]
async fn split_osc9_across_a_production_on_demand_snapshot_boundary_fires_at_most_once_and_never_stitches_into_a_later_detached_period()
 {
    let pane_id: PaneId = 40;
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));

    let pane = MuxPane::new_test(pane_id, 80, 24, output_target.clone());
    let (notif_tx, mut notif_rx) = mpsc::channel(8);
    *pane.notification_sender.lock().unwrap() = Some(notif_tx);

    // Field clones for the reader thread(s) below — mirrors
    // `register_pane_and_start_reader`'s own clone dance — taken BEFORE
    // `pane` is moved into the manager, so both the production handler
    // and the reader observe exactly this one pane's shared state.
    let shadow_parser = pane.shadow_parser.clone();
    let cwd = pane.cwd.clone();
    let title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let dims = pane.dims.clone();
    let output_capture = pane.output_capture.clone();

    let mgr = Arc::new(tokio::sync::Mutex::new(SessionManager::new()));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        sid
    };

    // chunk_a: OSC 9 introducer + partial body, no terminator.
    let chunk_a = b"\x1b]9;spl".to_vec();
    // chunk_b: rest of the body + terminator.
    let chunk_b = b"it-notice\x07".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let handle = std::thread::spawn({
        let output_target = output_target.clone();
        let shadow_parser = shadow_parser.clone();
        let cwd = cwd.clone();
        let title = title.clone();
        let title_sender = title_sender.clone();
        let notification_sender = notification_sender.clone();
        let agent_status_report_sender = agent_status_report_sender.clone();
        let raw_passthrough = raw_passthrough.clone();
        let passthrough_scanner = passthrough_scanner.clone();
        let scrollback = scrollback.clone();
        let dims = dims.clone();
        let output_capture = output_capture.clone();
        let chunks = vec![chunk_a.clone(), chunk_b.clone()];
        move || {
            pty_reader_loop(
                pane_id,
                Box::new(ScriptedReader::new(chunks)),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                output_capture,
            );
        }
    });

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on chunk_a");

    // Drive the PRODUCTION on-demand snapshot path while paused.
    let req = mux_ipc::protocol::MuxMessage {
        msg_type: mux_ipc::protocol::MessageType::RequestPaneSnapshot,
        pane_id,
        payload: Vec::new(),
    };
    let mut deferred = crate::mux::session::pane::DeferredOutputQueue::new();
    crate::mux::ipc::handlers::handle_request_pane_snapshot(
        &req,
        session_id,
        &mgr,
        &owned_tx,
        &mut deferred,
        10_000,
    )
    .await
    .expect("handle_request_pane_snapshot");
    assert!(
        deferred.is_empty(),
        "the channel has room; the snapshot must be enqueued immediately, \
         not deferred"
    );

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received.len(),
        4,
        "snapshot, chunk_a's replacement, chunk_b, EOF: {:?}",
        received
            .iter()
            .map(|c| (c.data.len(), c.kind))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        received[0].kind,
        crate::mux::session::pane::ChunkKind::Snapshot,
        "S must receive the production snapshot first"
    );
    assert_eq!(
        received[1].kind,
        crate::mux::session::pane::ChunkKind::PtyOutput
    );
    assert_eq!(
        received[2].kind,
        crate::mux::session::pane::ChunkKind::PtyOutput
    );
    assert!(received[3].data.is_empty(), "EOF chunk must follow");

    let mut after_snapshot = Vec::new();
    after_snapshot.extend_from_slice(&received[1].data);
    after_snapshot.extend_from_slice(&received[2].data);

    let complete_osc9 = b"\x1b]9;split-notice\x07";
    let occurrences = after_snapshot
        .windows(complete_osc9.len())
        .filter(|w| *w == complete_osc9)
        .count();
    assert_eq!(
        occurrences,
        1,
        "the bytes delivered to S after the snapshot must contain the \
         OSC 9 exactly once as a complete sequence: {:?}",
        String::from_utf8_lossy(&after_snapshot)
    );

    let mut notifications_before_detach = Vec::new();
    while let Ok((pid, msg)) = notif_rx.try_recv() {
        assert_eq!(pid, pane_id);
        notifications_before_detach.push(msg);
    }
    assert!(
        notifications_before_detach.len() <= 1,
        "at most one notification for the split OSC 9: {notifications_before_detach:?}"
    );
    assert_eq!(
        notifications_before_detach.len() + occurrences,
        1,
        "the total of daemon-side notifications plus complete OSC 9 \
         sequences delivered to S must be exactly one"
    );

    // AC-6: switch to Detached, then feed a bare terminator (no
    // introducer) followed by a SEPARATE complete OSC 9.
    *output_target.lock().unwrap() = PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    };

    let terminator_only = b"\x07".to_vec();
    let second_osc9 = b"\x1b]9;second-notice\x07".to_vec();

    let handle2 = std::thread::spawn({
        let output_target = output_target.clone();
        let shadow_parser = shadow_parser.clone();
        let cwd = cwd.clone();
        let title = title.clone();
        let title_sender = title_sender.clone();
        let notification_sender = notification_sender.clone();
        let agent_status_report_sender = agent_status_report_sender.clone();
        let raw_passthrough = raw_passthrough.clone();
        let passthrough_scanner = passthrough_scanner.clone();
        let scrollback = scrollback.clone();
        let dims = dims.clone();
        let output_capture = output_capture.clone();
        let chunks = vec![terminator_only, second_osc9];
        move || {
            pty_reader_loop(
                pane_id,
                Box::new(ScriptedReader::new(chunks)),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                output_capture,
            );
        }
    });
    handle2.join().unwrap();

    let mut notifications_after_detach = Vec::new();
    while let Ok((pid, msg)) = notif_rx.try_recv() {
        assert_eq!(pid, pane_id);
        notifications_after_detach.push(msg);
    }
    assert_eq!(
        notifications_after_detach.len(),
        1,
        "a bare terminator must fire nothing, and the SEPARATE complete \
         OSC 9 fed in the same Detached period must fire exactly once: \
         {notifications_after_detach:?}"
    );
    assert_eq!(notifications_after_detach[0], "second-notice");
}

/// AC-5, "Other side effects": title, agent-status report, OSC 133 marks
/// and OSC 7 cwd detection all run BEFORE the reader's forward decision
/// (unconditionally, per the reader flow's step 3), so a suppressed
/// chunk's side effects must be identical to the same chunk unsuppressed.
#[test]
fn side_effects_for_a_suppressed_chunk_match_the_same_chunk_unsuppressed() {
    fn run_once(pane_id: PaneId, suppress: bool) -> (Option<String>, String, Option<String>) {
        let output_target: SharedOutputTarget = Arc::new(StdMutex::new(
            PaneOutputTarget::Connected(mpsc::channel(16).0),
        ));
        let pane = MuxPane::new_test(pane_id, 80, 24, output_target);
        let output_capture = pane.output_capture.clone();

        let (title_tx, mut title_rx) = mpsc::channel(4);
        *pane.title_sender.lock().unwrap() = Some(title_tx);
        let (agent_tx, mut agent_rx) = mpsc::channel(16);
        *pane.agent_status_report_sender.lock().unwrap() = Some(agent_tx);

        let mut chunk = Vec::new();
        chunk.extend_from_slice(b"\x1b]2;My Title\x07");
        chunk.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=working\x07");
        chunk.extend_from_slice(b"\x1b]133;D\x07");
        chunk.extend_from_slice(b"\x1b]133;A\x07");
        chunk.extend_from_slice(b"\x1b]7;file://host/some/path\x07");

        if suppress {
            let (arrived_rx, release_tx) = output_capture.p2.arm();
            let (handle, tx, mut rx) = spawn_reader_with_chunks(&pane, vec![chunk]);
            arrived_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the reader must reach P2");
            output_capture.record_boundary(&tx, 1);
            release_tx.send(()).unwrap();
            handle.join().unwrap();
            while rx.try_recv().is_ok() {}
        } else {
            let (handle, _tx, mut rx) = spawn_reader_with_chunks(&pane, vec![chunk]);
            handle.join().unwrap();
            while rx.try_recv().is_ok() {}
        }

        let title = title_rx.try_recv().ok().map(|(_, t)| t);
        let mut items = Vec::new();
        while let Ok((_, item)) = agent_rx.try_recv() {
            items.push(item);
        }
        let cwd = pane.cwd.lock().unwrap().clone();
        (title, format!("{items:?}"), cwd)
    }

    let unsuppressed = run_once(10, false);
    let suppressed = run_once(11, true);
    assert_eq!(
        unsuppressed, suppressed,
        "title / agent-status+OSC133 items / OSC7 cwd must be identical \
         whether this chunk was suppressed or not"
    );
    // Sanity: the fixture must actually have produced observable side
    // effects, otherwise the equality above would be vacuous.
    assert!(unsuppressed.0.is_some(), "title must have been detected");
    assert!(
        unsuppressed.1.contains("Report") && unsuppressed.1.contains("Osc133Mark"),
        "agent-status report and OSC133 marks must have been detected: {}",
        unsuppressed.1
    );
    assert!(
        unsuppressed.2.is_some(),
        "OSC 7 cwd must have been detected"
    );
}

/// AC-7 (NFR2; TS-12), "Stress": the reader runs continuously against a
/// long, varied stream (plain text, device queries, alt-screen toggles, a
/// complete OSC 9, and incomplete-CSI tails) while a second thread
/// repeatedly performs on-demand-snapshot-shaped operations
/// (`captured_read` + `record_boundary`, racing the reader's own
/// suppression checks) and a third thread repeatedly calls
/// `MuxPane::resize` on a real PTY — all three sharing the SAME capture
/// exclusion. Bounded by iteration count; the whole test is ALSO bounded
/// by a wall-clock budget via bounded polling (never a bare `.join()`) so
/// a deadlock regression fails this ONE test with a clear message
/// instead of hanging the suite.
#[cfg(unix)]
#[test]
fn stress_reader_with_concurrent_resize_and_snapshot_paths_completes_within_budget() {
    let start = std::time::Instant::now();
    let budget = std::time::Duration::from_secs(30);

    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let placeholder_target: SharedOutputTarget = Arc::new(StdMutex::new(
        PaneOutputTarget::Connected(mpsc::channel(64).0),
    ));
    let mut pane = MuxPane::new(20, 80, 24, placeholder_target, writer, pair.master, None);

    let (tx, rx) = mpsc::channel::<PtyOutputChunk>(64);
    *pane.output_target.lock().unwrap() = PaneOutputTarget::Connected(tx.clone());

    let output_capture = pane.output_capture.clone();
    let output_target = pane.output_target.clone();
    let shadow_parser = pane.shadow_parser.clone();
    let cwd = pane.cwd.clone();
    let title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let dims = pane.dims.clone();

    let mut chunks = Vec::new();
    for i in 0..500u32 {
        chunks.push(match i % 5 {
            0 => format!("line {i}\r\n").into_bytes(),
            1 => b"\x1b[6n".to_vec(),
            2 => b"\x1b[?1049h\x1b[2Jtui frame\x1b[?1049l".to_vec(),
            3 => b"\x1b]9;stress-notice\x07".to_vec(),
            _ => b"\x1b[".to_vec(), // incomplete CSI tail
        });
    }

    let reader_handle = std::thread::spawn(move || {
        pty_reader_loop(
            20,
            Box::new(ScriptedReader::new(chunks)),
            output_target,
            shadow_parser,
            cwd,
            title,
            title_sender,
            notification_sender,
            agent_status_report_sender,
            raw_passthrough,
            passthrough_scanner,
            scrollback,
            dims,
            Arc::new(StdMutex::new(None)),
            output_capture,
        );
    });

    let snapshot_output_capture = pane.output_capture.clone();
    let snapshot_tx = tx.clone();
    let snapshot_handle = std::thread::spawn(move || {
        for _ in 0..200 {
            let (_, number) = snapshot_output_capture.captured_read(|| ());
            snapshot_output_capture.record_boundary(&snapshot_tx, number);
        }
    });

    let resize_handle = std::thread::spawn(move || {
        for i in 0..50u32 {
            let (cols, rows) = if i % 2 == 0 { (100, 30) } else { (80, 24) };
            let _ = pane.resize(cols, rows);
        }
        pane
    });

    // A concurrent drain, standing in for the real client connection's own
    // continuous receive loop — without this, the bounded channel fills
    // and the reader's `reserve_owned()` wait would never see a freed
    // slot until this test's OWN post-join drain ran, which is exactly
    // the self-inflicted deadlock this stress test must not manufacture.
    //
    // Stopped via an explicit flag rather than channel closure: `pane`
    // (returned by `resize_handle` and bound below) keeps its OWN clone of
    // `output_target` alive for the rest of this function, which itself
    // holds a `Sender` clone inside `PaneOutputTarget::Connected` — so the
    // channel never actually closes on its own within this test.
    drop(tx);
    let mut rx = rx;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let drain_stop = stop.clone();
    let drain_handle = std::thread::spawn(move || {
        loop {
            match rx.try_recv() {
                Ok(_) => continue,
                Err(mpsc::error::TryRecvError::Disconnected) => break,
                Err(mpsc::error::TryRecvError::Empty) => {
                    if drain_stop.load(std::sync::atomic::Ordering::SeqCst) {
                        while rx.try_recv().is_ok() {}
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
    });

    loop {
        if reader_handle.is_finished()
            && snapshot_handle.is_finished()
            && resize_handle.is_finished()
        {
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
            break;
        }
        if start.elapsed() > budget {
            panic!(
                "stress test threads did not finish within the {budget:?} \
                 budget — possible deadlock (reader={} snapshot={} resize={})",
                reader_handle.is_finished(),
                snapshot_handle.is_finished(),
                resize_handle.is_finished(),
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    reader_handle.join().unwrap();
    snapshot_handle.join().unwrap();
    let _pane = resize_handle.join().unwrap();

    while !drain_handle.is_finished() {
        if start.elapsed() > budget {
            panic!("the stand-in drain thread did not notice the stop signal in time");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drain_handle.join().unwrap();

    assert!(
        start.elapsed() < budget,
        "stress test exceeded its time budget: {:?}",
        start.elapsed()
    );
}

/// A `MasterPty` double whose `resize` always succeeds and does nothing
/// else. Stands in for a real OS PTY so
/// `snapshot_paths_run_concurrently_with_reader_and_resize_without_deadlock`
/// below can drive `MuxPane::resize` — FR12/TS-14's design constraint is
/// "no real PTY, no platform-specific process control" — without opening
/// one. Cross-platform: only the two `#[cfg(unix)]`-only trait methods are
/// gated, unlike `pane::tests::FailingResizeMaster`, which is gated
/// entirely and so is unix-only.
struct AlwaysResizableMaster;

impl portable_pty::MasterPty for AlwaysResizableMaster {
    fn resize(&self, _size: portable_pty::PtySize) -> Result<(), anyhow::Error> {
        Ok(())
    }
    fn get_size(&self) -> Result<portable_pty::PtySize, anyhow::Error> {
        Ok(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
    }
    fn try_clone_reader(&self) -> Result<Box<dyn std::io::Read + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("not supported in test double"))
    }
    fn take_writer(&self) -> Result<Box<dyn std::io::Write + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("not supported in test double"))
    }
    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }
    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        None
    }
}

/// AC-2 (FR12, NFR2; TS-14), stable_id `aca2b1d612ab97e0`: the three
/// PRODUCTION snapshot paths — `collect_reattach_data` (visible reattach),
/// `handle_request_pane_snapshot` (on-demand) and `resume_pane_with_permit`
/// (visibility resume, reached via `evaluate_output_target`'s hide step) —
/// run concurrently with the reader and a resize loop against the SAME
/// pane and the SAME destination channel (standing in for one client
/// connection, drained continuously so backpressure never stalls anything).
///
/// Unlike the older stand-in stress test above (which exercises
/// `captured_read` + `record_boundary` directly and needs a real PTY only
/// so `MuxPane::resize` has something to call), this test drives the real
/// production entry points end to end and needs no real PTY at all: the
/// resize loop runs against [`AlwaysResizableMaster`], a `MasterPty` double
/// that always succeeds — satisfying the design's "no real PTY, no
/// platform-specific process control" constraint, so this test runs
/// identically on Linux and Windows.
///
/// Bounded by iteration count on every concurrent loop; the whole test is
/// ALSO bounded by a wall-clock budget via bounded polling (never a bare
/// `.join()`/`.await`), so a deadlock regression fails this ONE test with a
/// clear message instead of hanging the suite. Deterministic in outcome,
/// not in interleaving (Test Notes): this asserts only completion (no
/// deadlock), no panic, that every `Snapshot`-kind chunk observed decodes,
/// and that each of the three production paths delivered successfully at
/// least once — never a specific interleaving order.
#[tokio::test]
async fn snapshot_paths_run_concurrently_with_reader_and_resize_without_deadlock() {
    let start = std::time::Instant::now();
    let budget = std::time::Duration::from_secs(30);

    let pane_id: PaneId = 90;
    let (dest_tx, dest_rx) = mpsc::channel::<PtyOutputChunk>(64);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(dest_tx.clone())));

    let pane = MuxPane::new(
        pane_id,
        80,
        24,
        output_target,
        Box::new(std::io::sink()),
        Box::new(AlwaysResizableMaster),
        None,
    );

    // Field clones for the reader thread, taken BEFORE `pane` moves into
    // the manager — mirrors `spawn_reader_with_chunks_in_session`'s own
    // clone dance, so both the production handlers (via the manager) and
    // the reader observe exactly this one pane's shared state.
    let output_target_for_reader = pane.output_target.clone();
    let shadow_parser = pane.shadow_parser.clone();
    let cwd = pane.cwd.clone();
    let title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let dims = pane.dims.clone();
    let output_capture = pane.output_capture.clone();

    let mgr = Arc::new(tokio::sync::Mutex::new(SessionManager::new()));
    let (session_id, window_id) = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        (sid, wid)
    };

    // A long, bounded stream: plain lines with occasional CSI device
    // queries (design's "text lines with occasional CSI device queries").
    let mut chunks = Vec::new();
    for i in 0..1000u32 {
        if i % 7 == 0 {
            chunks.push(b"\x1b[6n".to_vec());
        } else {
            chunks.push(format!("line {i}\r\n").into_bytes());
        }
    }

    let reader_handle = std::thread::spawn(move || {
        pty_reader_loop(
            pane_id,
            Box::new(ScriptedReader::new(chunks)),
            output_target_for_reader,
            shadow_parser,
            cwd,
            title,
            title_sender,
            notification_sender,
            agent_status_report_sender,
            raw_passthrough,
            passthrough_scanner,
            scrollback,
            dims,
            Arc::new(StdMutex::new(None)),
            output_capture,
        );
    });

    // One destination channel, standing in for one client connection,
    // drained continuously so backpressure never stalls the reader or any
    // of the concurrent snapshot paths below (mirrors the older stress
    // test's stand-in drain).
    let received: Arc<StdMutex<Vec<PtyOutputChunk>>> = Arc::new(StdMutex::new(Vec::new()));
    let drain_received = received.clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let drain_stop = stop.clone();
    let mut rx = dest_rx;
    let drain_handle = std::thread::spawn(move || {
        loop {
            match rx.try_recv() {
                Ok(chunk) => drain_received.lock().unwrap().push(chunk),
                Err(mpsc::error::TryRecvError::Disconnected) => break,
                Err(mpsc::error::TryRecvError::Empty) => {
                    if drain_stop.load(std::sync::atomic::Ordering::SeqCst) {
                        while let Ok(chunk) = rx.try_recv() {
                            drain_received.lock().unwrap().push(chunk);
                        }
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
    });

    let reattach_successes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let snapshot_successes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let resume_successes = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Loop 1: visible reattach through the PRODUCTION
    // `collect_reattach_data`, targeting the SAME destination.
    let reattach_handle = {
        let mgr = mgr.clone();
        let dest_tx = dest_tx.clone();
        let reattach_successes = reattach_successes.clone();
        tokio::spawn(async move {
            for _ in 0..50 {
                let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
                let (kick_tx, _kick_rx) = oneshot::channel::<()>();
                let data = crate::mux::ipc::reattach::collect_reattach_data(
                    &mgr, session_id, &dest_tx, &title_tx, kick_tx, true, 10_000,
                )
                .await;
                if data.len() == 1 {
                    reattach_successes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        })
    };

    // Loop 2: the PRODUCTION on-demand snapshot handler, targeting the
    // SAME destination.
    let snapshot_handle = {
        let mgr = mgr.clone();
        let dest_tx = dest_tx.clone();
        let snapshot_successes = snapshot_successes.clone();
        tokio::spawn(async move {
            for _ in 0..50 {
                let req = mux_ipc::protocol::MuxMessage {
                    msg_type: mux_ipc::protocol::MessageType::RequestPaneSnapshot,
                    pane_id,
                    payload: Vec::new(),
                };
                let mut deferred = crate::mux::session::pane::DeferredOutputQueue::new();
                let outcome = crate::mux::ipc::handlers::handle_request_pane_snapshot(
                    &req,
                    session_id,
                    &mgr,
                    &dest_tx,
                    &mut deferred,
                    10_000,
                )
                .await;
                if outcome.is_ok() && deferred.is_empty() {
                    snapshot_successes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        })
    };

    // Loop 3: hide via the PRODUCTION `evaluate_output_target`, then
    // resume via the PRODUCTION `resume_pane_with_permit` with a reserved
    // permit — both against the SAME destination. The permit is reserved
    // BEFORE either call, so nothing here holds `output_target` while
    // waiting on the channel (NFR2).
    let hide_resume_handle = {
        let mgr = mgr.clone();
        let dest_tx = dest_tx.clone();
        let resume_successes = resume_successes.clone();
        tokio::spawn(async move {
            for _ in 0..50 {
                let permit = match dest_tx.reserve().await {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let m = mgr.lock().await;
                let pane_ref = m
                    .get_session(session_id)
                    .unwrap()
                    .windows
                    .get(&window_id)
                    .unwrap()
                    .panes
                    .get(&pane_id)
                    .unwrap();
                let _ = crate::mux::session::pane::evaluate_output_target(
                    pane_ref, false, false, &dest_tx,
                );
                let outcome = crate::mux::session::pane::resume_pane_with_permit(
                    pane_ref,
                    &dest_tx,
                    crate::mux::session::pane::AnyPermit::Borrowed(permit),
                    10_000,
                );
                drop(m);
                if matches!(outcome, crate::mux::session::pane::ResumeOutcome::Resumed) {
                    resume_successes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        })
    };

    // Loop 4: the pane resize, alternating between two sizes — pure
    // contention against the same manager/pane locks, no real PTY (see
    // `AlwaysResizableMaster`), no platform-specific process control.
    let resize_handle = {
        let mgr = mgr.clone();
        tokio::spawn(async move {
            for i in 0..50u32 {
                let (cols, rows) = if i % 2 == 0 { (100, 30) } else { (80, 24) };
                let mut m = mgr.lock().await;
                if let Some(session) = m.get_session_mut(session_id) {
                    if let Some(window) = session.windows.get_mut(&window_id) {
                        if let Some(pane) = window.panes.get_mut(&pane_id) {
                            let _ = pane.resize(cols, rows);
                        }
                    }
                }
            }
        })
    };

    loop {
        if reader_handle.is_finished()
            && reattach_handle.is_finished()
            && snapshot_handle.is_finished()
            && hide_resume_handle.is_finished()
            && resize_handle.is_finished()
        {
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
            break;
        }
        if start.elapsed() > budget {
            panic!(
                "concurrent snapshot-paths threads did not finish within the \
                 {budget:?} budget — possible deadlock (reader={} reattach={} \
                 snapshot={} hide_resume={} resize={})",
                reader_handle.is_finished(),
                reattach_handle.is_finished(),
                snapshot_handle.is_finished(),
                hide_resume_handle.is_finished(),
                resize_handle.is_finished(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    reader_handle.join().expect("reader thread must not panic");
    reattach_handle.await.expect("reattach task must not panic");
    snapshot_handle.await.expect("snapshot task must not panic");
    hide_resume_handle
        .await
        .expect("hide/resume task must not panic");
    resize_handle.await.expect("resize task must not panic");

    while !drain_handle.is_finished() {
        if start.elapsed() > budget {
            panic!("the stand-in drain thread did not notice the stop signal in time");
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    drain_handle.join().expect("drain thread must not panic");

    assert!(
        start.elapsed() < budget,
        "concurrent snapshot-paths test exceeded its time budget: {:?}",
        start.elapsed()
    );

    // Every Snapshot-kind chunk observed must decode as a structured
    // payload (never Malformed — this daemon always encodes via
    // `encode_snapshot_segments`, which always emits the magic prefix).
    let received = received.lock().unwrap();
    let mut snapshot_chunk_count = 0usize;
    for chunk in received.iter() {
        if chunk.kind == crate::mux::session::pane::ChunkKind::Snapshot {
            snapshot_chunk_count += 1;
            let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&chunk.data);
            assert!(
                matches!(
                    decoded,
                    mux_ipc::protocol::DecodedSnapshotPayload::Structured { .. }
                ),
                "every Snapshot-kind chunk received must decode as a \
                 structured payload"
            );
        }
    }
    assert!(
        snapshot_chunk_count > 0,
        "at least one Snapshot-kind chunk must have been observed across \
         the concurrent runs"
    );

    // Each of the three PRODUCTION snapshot paths must have completed at
    // least one successful delivery.
    assert!(
        reattach_successes.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "collect_reattach_data must have delivered at least once"
    );
    assert!(
        snapshot_successes.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "handle_request_pane_snapshot must have delivered at least once"
    );
    assert!(
        resume_successes.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "resume_pane_with_permit must have delivered at least once"
    );
}

// ── FR13 (task0005): DECISIONS.md document-contract checks. These read
// the checked-in `feature-docs/mux-suppressed-output-fixes/DECISIONS.md`
// at compile time so AC-3/AC-4's content requirements are covered by an
// executable check rather than by inspection alone. `feature-docs/
// mux-suppressed-output-fixes/**` is one of this feature's own declared
// paths, so embedding it here is reading within the feature's own scope.

const DECISIONS_MD: &str =
    include_str!("../../../../../feature-docs/mux-suppressed-output-fixes/DECISIONS.md");

/// The 17 stable_ids the DECISIONS.md table must list as exactly one row
/// each. `9e6a468b3a45ceeb` is deliberately excluded — it has its own
/// two-row assertion below.
const SINGLY_LISTED_STABLE_IDS: [&str; 17] = [
    "19209420de72b144",
    "61d33252f22b6fd2",
    "8d069c589dc21784",
    "66d05376ff960d53",
    "001161ab9fa3c20b",
    "c8aa5052b1a02acd",
    "66170217dc5057ce",
    "3bc1e21fdd8702ef",
    "30ee5a7036a7fc6e",
    "830f950f39e499fa",
    "9a548939524b405a",
    "29ff65b6c01032dc",
    "39267160fcf1bb2a",
    "692921cdd030612d",
    "9a7dc7697c6af992",
    "5988c2406aa06a7b",
    "aca2b1d612ab97e0",
];

/// Every `(stable_id cell, full row line)` pair from DECISIONS.md's
/// judgement table — lines starting with `| ` that are neither the header
/// nor the `|---|` separator row. Scoped to the TABLE's first column
/// (rather than a whole-document substring search) so a stable_id
/// mentioned in another row's prose (e.g. "692921cdd030612d と同一箇所")
/// for cross-reference purposes is never mistaken for a second row.
fn decisions_md_rows() -> Vec<(String, &'static str)> {
    DECISIONS_MD
        .lines()
        .filter(|l| l.starts_with("| ") && !l.contains("---"))
        .map(|l| {
            let cell = l
                .trim_start_matches('|')
                .split('|')
                .next()
                .unwrap()
                .trim()
                .to_string();
            (cell, l)
        })
        // Exclude the header row by an EXACT match on its first cell
        // ("stable_id") rather than a substring search over the whole
        // line — a data row's own rationale prose may legitimately use
        // the word "stable_id" (e.g. "同一 stable_id で2件の指摘がある"),
        // and a substring filter would wrongly drop that row too.
        .filter(|(cell, _)| cell != "stable_id")
        .collect()
}

/// AC-4: all 18 stable_ids from the IMPLEMENTATION.md D7 registry are
/// listed, and only those, each exactly once — except `9e6a468b3a45ceeb`,
/// which has one row per target (viewer launch, inline image), so exactly
/// twice. 17 + 2 = 19 rows total.
#[test]
fn decisions_md_lists_every_d7_stable_id_exactly_once_except_the_split_row() {
    let rows = decisions_md_rows();
    assert_eq!(rows.len(), 19, "expected 19 table rows: {rows:?}");
    for id in SINGLY_LISTED_STABLE_IDS {
        let count = rows.iter().filter(|(cell, _)| cell == id).count();
        assert_eq!(
            count, 1,
            "stable_id {id} must appear as exactly one row: {rows:?}"
        );
    }
    let split_count = rows
        .iter()
        .filter(|(cell, _)| cell.starts_with("9e6a468b3a45ceeb"))
        .count();
    assert_eq!(
        split_count, 2,
        "9e6a468b3a45ceeb must have exactly two rows: {rows:?}"
    );
}

/// AC-4: every row has a verdict of 対応済み or 対応不要, and a non-empty
/// rationale + regression-test cell (the table has 5 columns: stable_id,
/// requirement, verdict, rationale, regression tests).
#[test]
fn decisions_md_every_row_has_a_verdict_and_nonempty_rationale_and_tests() {
    for (cell, line) in decisions_md_rows() {
        let cols: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(
            cols.len(),
            5,
            "row for {cell} must have 5 columns (stable_id, requirement, \
             verdict, rationale, regression tests): {line}"
        );
        let verdict = cols[2];
        assert!(
            verdict.starts_with("対応済み") || verdict.starts_with("対応不要"),
            "row for {cell} must have a verdict of 対応済み or 対応不要: {verdict}"
        );
        assert!(
            !cols[3].is_empty(),
            "row for {cell} must have a non-empty rationale"
        );
        assert!(
            !cols[4].is_empty(),
            "row for {cell} must have a non-empty regression-tests cell"
        );
    }
}

/// AC-3: the three FR12 "already addressed" stable_ids are recorded as
/// 対応済み (never 対応不要).
#[test]
fn decisions_md_marks_fr12_already_addressed_stable_ids_as_addressed() {
    let rows = decisions_md_rows();
    for id in ["692921cdd030612d", "9a7dc7697c6af992", "5988c2406aa06a7b"] {
        let (_, line) = rows
            .iter()
            .find(|(cell, _)| cell == id)
            .unwrap_or_else(|| panic!("no DECISIONS.md row found for stable_id {id}"));
        assert!(
            line.contains("対応済み"),
            "stable_id {id}'s row must be verdict 対応済み: {line}"
        );
    }
}

/// AC-4: `9e6a468b3a45ceeb` records the target-specific verdicts the task
/// plan requires as two DISTINCT rows — viewer launch is 対応済み, inline
/// image is 対応不要.
#[test]
fn decisions_md_splits_9e6a468b3a45ceeb_into_addressed_viewer_launch_and_unaddressed_inline_image()
{
    let rows: Vec<(String, &str)> = decisions_md_rows()
        .into_iter()
        .filter(|(cell, _)| cell.starts_with("9e6a468b3a45ceeb"))
        .collect();
    assert_eq!(rows.len(), 2, "expected exactly two rows: {rows:?}");
    assert!(
        rows.iter()
            .any(|(_, l)| l.contains("ビューア起動") && l.contains("対応済み")),
        "the viewer-launch row must be 対応済み: {rows:?}"
    );
    assert!(
        rows.iter()
            .any(|(_, l)| l.contains("インライン画像") && l.contains("対応不要")),
        "the inline-image row must be 対応不要: {rows:?}"
    );
}

/// AC-4: the known-gap register (IMPLEMENTATION.md D6) is carried —
/// inline images, the N-byte retained-window gap, and the OSC-number u16
/// overflow are all named.
#[test]
fn decisions_md_carries_the_known_gap_register() {
    for needle in ["Kitty APC", "SIXEL DCS", "256 バイト", "u16"] {
        assert!(
            DECISIONS_MD.contains(needle),
            "DECISIONS.md's known-gap register must mention {needle:?}"
        );
    }
}

/// AC-4 (task0006): the IMPLEMENTATION.md D7 registry's test-name list —
/// SHARED, never duplicated, between
/// `decisions_md_contains_every_d7_registry_test_name` (every name is
/// cited in DECISIONS.md) and
/// `every_d7_registry_test_name_is_defined_as_a_test_function_in_crate_source`
/// (every name actually names a test function under `src-tauri/src`).
/// Moving this list into a shared constant is a behavior-preserving change
/// to the pre-existing `decisions_md_contains_every_d7_registry_test_name`
/// (task0006 completion report).
const D7_REGISTRY_TEST_NAMES: [&str; 23] = [
    "suppressed_alt_osc_color_query_split_inside_st_is_delivered_once",
    "write_filter_holds_string_ending_in_trailing_esc_until_completed",
    "replacement_never_extracts_a_query_consumed_as_charset_designator",
    "replacement_matches_client_reference_for_transition_corpus",
    "color_query_predicate_matches_client_osc_number_and_theme_rules",
    "incomplete_csi_tail_is_resent_without_executed_c0_controls",
    "pending_does_not_drop_alt_region_queries_viewer_launches_or_own_tail",
    "viewer_launch_in_suppressed_chunk_is_delivered_exactly_once",
    "write_filter_closes_esc_aborted_strings_and_holds_only_incomplete_tail",
    "suppressed_chunk_after_aborted_strings_does_not_resend_delivered_query_or_text",
    "query_split_across_reads_is_answered_once_at_every_split_position",
    "utf8_split_across_reads_never_prints_a_replacement_character",
    "consecutive_suppressed_chunks_leave_next_forwarded_chunk_intact",
    "inline_images_in_suppressed_chunk_are_not_delivered",
    "viewer_launch_image_kind_is_not_redelivered_d3",
    "visibility_resume_restores_alt_screen_mode_and_content",
    "visibility_resume_after_hidden_alt_exit_shows_main_screen",
    "visibility_resume_keeps_main_pane_progress_bar_layout",
    "suppressed_queries_arrive_after_snapshot_in_order_once_each",
    "evaluate_output_target_never_resumes_a_detached_pane",
    "production_visible_resume_delivers_snapshot_before_replacement_and_next_chunk",
    "split_osc9_across_two_suppressed_chunks_never_fires_more_than_once",
    "snapshot_paths_run_concurrently_with_reader_and_resize_without_deadlock",
];

/// AC-4: every regression-test name from the IMPLEMENTATION.md D7 registry
/// is cited somewhere in DECISIONS.md — a transcription check that would
/// catch a dropped or misspelled test name.
#[test]
fn decisions_md_contains_every_d7_registry_test_name() {
    for name in D7_REGISTRY_TEST_NAMES {
        assert!(
            DECISIONS_MD.contains(name),
            "DECISIONS.md must cite the D7 registry test name {name:?}"
        );
    }
}

/// Recursively search `dir` (and its subdirectories) for a `.rs` file
/// whose text defines a function named `name` — `fn NAME(` at a word
/// boundary (not a prefix of a longer identifier). Platform-neutral: uses
/// `std::path::Path`/`std::fs::read_dir` throughout, no hardcoded path
/// separators.
fn source_tree_defines_function(dir: &std::path::Path, name: &str) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if source_tree_defines_function(&path, name) {
                return true;
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            if file_defines_function(&content, name) {
                return true;
            }
        }
    }
    false
}

/// Whether `content` contains a `fn NAME(` definition, where the
/// character immediately following `NAME` (before any `(`) is never an
/// identifier character — so a search for `foo` never matches a
/// definition of `foo_bar`.
fn file_defines_function(content: &str, name: &str) -> bool {
    let needle = format!("fn {name}");
    let mut search_from = 0;
    while let Some(pos) = content[search_from..].find(&needle) {
        let match_end = search_from + pos + needle.len();
        let boundary_ok = content[match_end..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric() && c != '_')
            .unwrap_or(true);
        if boundary_ok && content[match_end..].trim_start().starts_with('(') {
            return true;
        }
        search_from = match_end;
    }
    false
}

/// AC-4 (FR13; TS-21): the lookup `every_d7_registry_test_name_is_defined_as_a_test_function_in_crate_source`
/// below drives — a name is "defined" only if some `.rs` file under this
/// crate's `src` (found from the crate manifest directory, via
/// `CARGO_MANIFEST_DIR`, never a hardcoded path) actually defines a
/// function by that name.
fn registry_test_name_is_defined_in_crate_source(name: &str) -> bool {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    source_tree_defines_function(&src_dir, name)
}

/// AC-4 (FR13; TS-21): every name on the D7 registry list — the SAME list
/// `decisions_md_contains_every_d7_registry_test_name` uses, never a
/// second copy — must actually be a test function defined somewhere under
/// `src-tauri/src`, not merely a name DECISIONS.md happens to mention.
#[test]
fn every_d7_registry_test_name_is_defined_as_a_test_function_in_crate_source() {
    for name in D7_REGISTRY_TEST_NAMES {
        assert!(
            registry_test_name_is_defined_in_crate_source(name),
            "D7 registry test name {name:?} is not defined as a function \
             anywhere under src-tauri/src"
        );
    }
}

/// AC-4: the lookup itself is negative-sensitive — a name that is
/// deliberately never defined anywhere must be reported missing, so this
/// test's own pass state never depends on a name that merely happens to
/// be absent by accident (and a lookup that vacuously always returns
/// `true` would be caught here).
#[test]
fn registry_test_name_lookup_reports_a_deliberately_undefined_name_as_missing() {
    assert!(
        !registry_test_name_is_defined_in_crate_source(
            "definitely_not_a_real_test_function_zzzzz_task0006"
        ),
        "the lookup must report a name that is never defined as missing"
    );
}

// ── task0004 round-4 rework (D1'): dimensions travel as structural
// segments (`term_core::terminal_core::ReplaySegment`) alongside the
// payload — never as an in-band `OSC 777;emterm;resize;…` marker byte
// sequence. Rounds 1-3's marker-scanning fix (IMPLEMENTATION.md D1,
// `tmp/apt-progress-bar-regression-2026-07-09.md` PROBE D) is superseded:
// the coordinate-drift regression tests below now construct explicit
// segment lists instead of embedding marker bytes, and the forgery tests
// (AC-1) prove marker-SHAPED byte sequences carry no authority at all —
// whatever survives the write-path strip (now identical for every kind,
// see `strip_pty_output_for_scrollback_write`'s doc comment) becomes
// ordinary content with no bearing on replay dimensions, because nothing
// scans for one any more.

/// apt-style scroll-region + bottom-bar recording (mirrors PROBE D's
/// synthesis, `install-progress.cc`'s SIGWINCH re-setup behavior). No
/// marker bytes are embedded; the caller supplies the segment
/// boundaries explicitly, mirroring what `ScrollbackRingBuffer::
/// read_segments` would report for a real `MuxPane::new` (initial dims)
/// + `MuxPane::resize` (mid-run SIGWINCH) sequence.
fn synth_apt_bytes_with_midrun_resize(
    cols: u16,
    rows_a: u16,
    rows_b: u16,
) -> (Vec<u8>, Vec<ReplaySegment>) {
    synth_apt_bytes_with_midrun_resize_inner(cols, rows_a, rows_b, true)
}

/// task0002 AC-1: region-active variant of
/// [`synth_apt_bytes_with_midrun_resize`] — byte-for-byte identical up to
/// and including the rows_b progress-bar updates, but STOPS before apt's
/// own stop sequence (the trailing `CSI r` that resets the scroll region
/// to the full screen), so apt's scroll region is still active when the
/// caller snapshots this stream.
fn synth_apt_bytes_with_midrun_resize_region_active(
    cols: u16,
    rows_a: u16,
    rows_b: u16,
) -> (Vec<u8>, Vec<ReplaySegment>) {
    synth_apt_bytes_with_midrun_resize_inner(cols, rows_a, rows_b, false)
}

/// Shared body for [`synth_apt_bytes_with_midrun_resize`] (`include_stop
/// = true`) and [`synth_apt_bytes_with_midrun_resize_region_active`]
/// (`include_stop = false`).
fn synth_apt_bytes_with_midrun_resize_inner(
    cols: u16,
    rows_a: u16,
    rows_b: u16,
    include_stop: bool,
) -> (Vec<u8>, Vec<ReplaySegment>) {
    let mut b = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols,
        rows: rows_a,
    }];
    // Fill history so the cursor starts at the bottom, matching a real
    // terminal's state when a long-running command begins.
    for i in 0..rows_a.max(rows_b) + 20 {
        b.extend_from_slice(format!("history line {i} filling the screen\r\n").as_bytes());
    }
    b.extend_from_slice(b"$ sudo apt reinstall ./build/emterm.deb\r\n");
    // Start (rows_a-shaped scroll region + bottom bar).
    b.extend_from_slice(b"\n\x1b7");
    b.extend_from_slice(format!("\x1b[0;{}r", rows_a - 1).as_bytes());
    b.extend_from_slice(b"\x1b8\x1b[1A");
    for pct in [0u32, 15, 30] {
        b.extend_from_slice(
            format!("emterm log line at {pct} percent unpacking something\r\n").as_bytes(),
        );
        let filled = (pct as usize * 60) / 100;
        b.extend_from_slice(
            format!(
                "\x1b7\x1b[{rows_a};0f\x1b[42m\x1b[30m進捗: [{pct:3}%] [{}{}]\x1b[49m\x1b[39m\x1b[0m\x1b8",
                "\u{2588}".repeat(filled),
                " ".repeat(60 - filled),
            )
            .as_bytes(),
        );
    }
    // SIGWINCH: the daemon records a segment for rows_b at THIS offset
    // (this is what `ScrollbackRingBuffer::write_resize_marker` records
    // via `MuxPane::resize`), then apt re-sets up the scroll region +
    // bar for rows_b.
    segments.push(ReplaySegment {
        offset: b.len() as u32,
        cols,
        rows: rows_b,
    });
    b.extend_from_slice(b"\n\x1b7");
    b.extend_from_slice(format!("\x1b[0;{}r", rows_b - 1).as_bytes());
    b.extend_from_slice(b"\x1b8\x1b[1A");
    for pct in [45u32, 60, 75, 90, 100] {
        b.extend_from_slice(
            format!("emterm log line at {pct} percent installing package\r\n").as_bytes(),
        );
        let filled = (pct as usize * 60) / 100;
        b.extend_from_slice(
            format!(
                "\x1b7\x1b[{rows_b};0f\x1b[42m\x1b[30m進捗: [{pct:3}%] [{}{}]\x1b[49m\x1b[39m\x1b[0m\x1b8",
                "\u{2588}".repeat(filled),
                " ".repeat(60 - filled),
            )
            .as_bytes(),
        );
    }
    if include_stop {
        // Stop (rows_b-shaped).
        b.extend_from_slice(b"\x1b7");
        b.extend_from_slice(format!("\x1b[0;{rows_b}r").as_bytes());
        b.extend_from_slice(b"\x1b8\x1b[J");
        b.extend_from_slice(b"$ done\r\n");
    }
    (b, segments)
}

/// AC-2: the apt-style recording, replayed through the full snapshot
/// pipeline into a fixed-size core, produces ZERO rows mixing bar
/// fragments with log-line content — fails when the segment authority
/// is dropped (PROBE D observed 1-3 tainted rows per case for the same
/// synthesis without dimension attribution).
#[test]
fn apt_style_recording_replays_without_cross_line_mixing() {
    use term_core::terminal_core::TerminalCore;
    let cols: u16 = 120;
    for (rec_a, rec_b, replay_rows) in [
        (47u16, 48u16, 47u16),
        (47, 48, 48),
        (48, 47, 47),
        (48, 47, 48),
    ] {
        let (recording, segments) = synth_apt_bytes_with_midrun_resize(cols, rec_a, rec_b);
        let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
            &recording,
            &to_tuples(&segments),
            b"",
            false,
            (cols, replay_rows),
        );
        let mut core = TerminalCore::new(cols, replay_rows, 10_000);
        core.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));
        let mut tainted = Vec::new();
        for r in 0..replay_rows {
            let line = core.get_line_text(r);
            let has_bar = line.contains('\u{2588}') || line.trim_end().ends_with(']');
            let has_log = line.contains("percent");
            if has_bar && has_log {
                tainted.push(format!("row {r}: {line}"));
            }
        }
        assert!(
            tainted.is_empty(),
            "rec {rec_a}->{rec_b} replay@{replay_rows}: expected zero cross-line-mixed rows \
             with segment attribution, got {tainted:?}"
        );
    }
}

/// AC-5(a) (mux-snapshot-ring-wrap-restore task0001): the apt synthesizer,
/// pushed past a genuinely wrapped ring's capacity (filler bytes in
/// front, per the task plan's Test Notes), snapshotted via the wrap-aware
/// builder. The replayed visible region must equal the shadow parser's
/// screen row by row — FIDELITY, not a zero-mixing guarantee: a real
/// `vt100::Parser` fed this exact apt fixture (mirroring
/// `apt_style_recording_replays_without_cross_line_mixing`'s detector)
/// already shows residual mixed rows of its OWN before any dump-block
/// code runs at all (confirmed by direct inspection against the bare
/// `vt100` crate, independent of filler, resize, or this feature — a
/// short log line printed via `\x1b[1A` cursor-up right after a longer
/// bar draw leaves a trailing fragment vt100 does not erase). That is
/// exactly NFR6 "the shadow parser can itself carry residual trashed
/// cells from unrelated bugs (not repaired by this feature)" territory:
/// the dump block's job is to faithfully MIRROR whatever the shadow
/// parser currently shows, mixed or not, never to repair it — so the
/// invariant this test pins is `client_mixed == shadow_mixed` (never
/// MORE), not `== 0`.
///
/// AC-6's continued-output claim is deliberately NOT exercised with this
/// apt fixture: the natural "reference" oracle would be a `TerminalCore`
/// fed the raw apt bytes directly, but `term_core` and the real `vt100`
/// crate handle this exact short-line-after-cursor-up pattern DIFFERENTLY
/// (confirmed by direct inspection — `term_core` does not reproduce the
/// vt100 crate's residual fragment), so such a reference would not agree
/// with the (deliberately vt100-shadow-faithful) client even absent this
/// feature — a cross-engine confound, not a regression to catch. AC-6 is
/// instead proven with a clean, single-engine (`term_core` on both sides)
/// fixture: `build_snapshot_bytes_for_ring_continued_output_matches_a_reference_fed_the_whole_stream`
/// in `snapshot_bytes/wrap_restore_tests.rs`.
///
/// Held at a SINGLE dims throughout (`synth_apt_bytes_with_midrun_resize`
/// called with `rows_a == rows_b`, so its own internal "resize" section is
/// a same-dims no-op) so the fixture isolates this apt-specific mixing
/// characteristic from resize-driven effects. The resize-differs-from-
/// current-dims case itself is covered separately, with controlled
/// (non-apt) content, by
/// `build_snapshot_bytes_for_ring_uses_current_dims_even_when_it_differs_from_the_last_ring_segment`
/// in `snapshot_bytes/wrap_restore_tests.rs`.
#[test]
fn apt_style_recording_past_a_wrapped_ring_restores_the_shadow_parsers_screen_faithfully() {
    use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
    use term_core::terminal_core::TerminalCore;

    let cols: u16 = 120;
    let rows: u16 = 48;

    let (apt_bytes, apt_segments) = synth_apt_bytes_with_midrun_resize(cols, rows, rows);
    assert_eq!(apt_segments.len(), 2, "test prerequisite");

    // Filler bytes in front so the ring wraps well before apt starts —
    // plain unrelated history, evicted first, never touched by the mixing
    // detector.
    let mut filler = Vec::new();
    for i in 0..400u32 {
        filler.extend_from_slice(format!("filler line {i} of unrelated history\r\n").as_bytes());
    }

    let capacity = apt_bytes.len() + filler.len() / 4; // wraps: evicts most filler, keeps all of apt
    let mut rb = ScrollbackRingBuffer::new(capacity);
    let mut shadow = vt100::Parser::new(rows, cols, 0);

    rb.attribute_write(cols, rows, &filler);
    shadow.process(&filler);
    rb.attribute_write(cols, rows, &apt_bytes);
    shadow.process(&apt_bytes);

    let (raw, segments, ring_wrapped) = rb.read_segments_with_wrap_state();
    assert!(
        ring_wrapped,
        "test prerequisite: the ring must have wrapped"
    );

    let shadow_mixed = {
        let mut count = 0;
        for r in 0..rows {
            let line = shadow
                .screen()
                .rows(0, cols)
                .nth(r as usize)
                .unwrap_or_default();
            let has_bar = line.contains('\u{2588}') || line.trim_end().ends_with(']');
            let has_log = line.contains("percent");
            if has_bar && has_log {
                count += 1;
            }
        }
        count
    };

    let screen_dump = shadow.screen().contents_formatted();
    let (payload, out_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &screen_dump,
        false,
        ring_wrapped,
        (cols, rows),
        10_000,
    );

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    let client_mixed = count_mixed_rows(&client, rows);
    assert_eq!(
        client_mixed, shadow_mixed,
        "post-wrap snapshot replay must introduce zero ADDITIONAL cross-\
         line-mixed rows beyond whatever the shadow parser's own screen \
         already has (NFR6: mirror it faithfully, never repair or worsen \
         it)"
    );
    for r in 0..rows {
        let got = client.get_line_text(r);
        let want = shadow
            .screen()
            .rows(0, cols)
            .nth(r as usize)
            .unwrap_or_default();
        assert_eq!(got.trim_end(), want.trim_end(), "row {r} mismatch");
    }
}

/// Test-only oracle mirroring `ScrollbackRingBuffer::read_segments`'s
/// head/mid split, but over a marker history that was NEVER capped to
/// `MAX_DIM_MARKERS` — the theoretical FULL attribution the real cap
/// only approximates. Unlike the real method this never needs a
/// cap-eviction fallback: the full history's very first marker (offset
/// 0) always qualifies as `head` (`0 <= oldest_offset` always holds),
/// so there is no gap to attribute at all.
fn full_attribution_segments(
    markers: &[(u64, u16, u16)],
    oldest_offset: u64,
    raw_len: usize,
) -> Vec<(usize, u16, u16)> {
    let mut head: Option<(u16, u16)> = None;
    let mut mid: Vec<(usize, u16, u16)> = Vec::new();
    for &(offset, cols, rows) in markers {
        if offset <= oldest_offset {
            head = Some((cols, rows));
        } else {
            let pos = ((offset - oldest_offset) as usize).min(raw_len);
            mid.push((pos, cols, rows));
        }
    }
    let mut segments = Vec::with_capacity(mid.len() + 1);
    if let Some((cols, rows)) = head {
        segments.push((0usize, cols, rows));
    }
    segments.extend(mid);
    segments
}

/// Counts rows in `core`'s current viewport (`0..target_rows`) that mix
/// a progress-bar fragment with log-line content — the cross-line-
/// mixing shape `apt_style_recording_replays_without_cross_line_mixing`
/// and PROBE D both use.
fn count_mixed_rows(core: &term_core::terminal_core::TerminalCore, target_rows: u16) -> usize {
    let mut count = 0;
    for r in 0..target_rows {
        let line = core.get_line_text(r);
        let has_bar = line.contains('\u{2588}') || line.trim_end().ends_with(']');
        let has_log = line.contains("percent");
        if has_bar && has_log {
            count += 1;
        }
    }
    count
}

/// AC-1 (FR2, FR7, FR8, FR9; TS-3, task0002 D8): the apt-style stream with
/// a REAL mid-run resize (`rows_a != rows_b`) past a wrapped ring's
/// capacity, snapshotted both while apt's scroll region is still active
/// (the region-active fixture variant) AND after apt's stop sequence. The
/// ring and the shadow parser are resized in lockstep at the segment
/// boundary — unlike
/// `apt_style_recording_past_a_wrapped_ring_restores_the_shadow_parsers_screen_faithfully`
/// (task0001, held at a single dims), this exercises the D8 probe-history
/// fix directly: a real resize happens INSIDE the pre-dump replay.
#[test]
fn apt_style_resize_and_wrap_restores_replay_state_region_active_and_after_stop() {
    use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
    use term_core::terminal_core::{MODE_ORIGIN, TerminalCore};

    let cols: u16 = 120;

    for (rows_a, rows_b) in [(47u16, 48u16), (48u16, 47u16)] {
        for region_active in [true, false] {
            let (apt_bytes, apt_segments) = if region_active {
                synth_apt_bytes_with_midrun_resize_region_active(cols, rows_a, rows_b)
            } else {
                synth_apt_bytes_with_midrun_resize(cols, rows_a, rows_b)
            };
            assert_eq!(
                apt_segments.len(),
                2,
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}: test prerequisite"
            );
            let resize_offset = apt_segments[1].offset as usize;

            // Filler bytes in front so the ring wraps well before apt
            // starts, mirroring task0001's fixture.
            let mut filler = Vec::new();
            for i in 0..400u32 {
                filler.extend_from_slice(
                    format!("filler line {i} of unrelated history\r\n").as_bytes(),
                );
            }

            let capacity = apt_bytes.len() + filler.len() / 4;
            let mut rb = ScrollbackRingBuffer::new(capacity);
            let mut shadow = vt100::Parser::new(rows_a, cols, 0);

            rb.attribute_write(cols, rows_a, &filler);
            shadow.process(&filler);

            // The ring and the shadow parser are resized IN LOCKSTEP at
            // the segment boundary, mirroring a real `MuxPane::resize`
            // (SIGWINCH mid-run).
            rb.attribute_write(cols, rows_a, &apt_bytes[..resize_offset]);
            shadow.process(&apt_bytes[..resize_offset]);
            rb.attribute_write(cols, rows_b, &apt_bytes[resize_offset..]);
            shadow.screen_mut().set_size(rows_b, cols);
            shadow.process(&apt_bytes[resize_offset..]);

            let (raw, segments, ring_wrapped) = rb.read_segments_with_wrap_state();
            assert!(
                ring_wrapped,
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}: \
                 test prerequisite: the ring must have wrapped"
            );

            let current_dims = (cols, rows_b);
            let screen_dump = shadow.screen().contents_formatted();
            let (payload, out_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes_for_ring(
                &raw,
                &segments,
                &screen_dump,
                false,
                ring_wrapped,
                current_dims,
                10_000,
            );

            let last = *out_segments
                .last()
                .expect("the dump block must add a trailing segment");
            assert_eq!(
                (last.1, last.2),
                current_dims,
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}: \
                 decoded segments must end with (dump block start, current_dims)"
            );

            let mut client = TerminalCore::new(cols, rows_b, 10_000);
            client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

            for r in 0..rows_b {
                let got = client.get_line_text(r);
                let want = shadow
                    .screen()
                    .rows(0, cols)
                    .nth(r as usize)
                    .unwrap_or_default();
                assert_eq!(
                    got.trim_end(),
                    want.trim_end(),
                    "rows_a={rows_a} rows_b={rows_b} region_active={region_active}: row {r} mismatch"
                );
            }

            let shadow_mixed = {
                let mut count = 0;
                for r in 0..rows_b {
                    let line = shadow
                        .screen()
                        .rows(0, cols)
                        .nth(r as usize)
                        .unwrap_or_default();
                    let has_bar = line.contains('\u{2588}') || line.trim_end().ends_with(']');
                    let has_log = line.contains("percent");
                    if has_bar && has_log {
                        count += 1;
                    }
                }
                count
            };
            let client_mixed = count_mixed_rows(&client, rows_b);
            assert_eq!(
                client_mixed, shadow_mixed,
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}: \
                 client mixed-row count must equal the shadow parser's own"
            );

            let dump_start = last.0;
            let truncated_payload = &payload[..dump_start];
            let truncated_segments: Vec<(usize, u16, u16)> = out_segments
                .iter()
                .copied()
                .filter(|&(offset, _, _)| offset < dump_start)
                .collect();
            let mut oracle = TerminalCore::new(cols, rows_b, 10_000);
            oracle.reset_and_replay_segments(
                truncated_payload,
                &to_replay_segments(&truncated_segments),
            );

            assert_eq!(
                client.get_scroll_region_top(),
                oracle.get_scroll_region_top(),
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}"
            );
            assert_eq!(
                client.get_scroll_region_bottom(),
                oracle.get_scroll_region_bottom(),
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}"
            );
            assert_eq!(
                client.get_mode(MODE_ORIGIN),
                oracle.get_mode(MODE_ORIGIN),
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}"
            );
            assert_eq!(
                client.get_cursor_row(),
                oracle.get_cursor_row(),
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}"
            );
            assert_eq!(
                client.get_cursor_col(),
                oracle.get_cursor_col(),
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}"
            );
            assert_eq!(
                client.get_wrap_pending(),
                oracle.get_wrap_pending(),
                "rows_a={rows_a} rows_b={rows_b} region_active={region_active}"
            );
        }
    }
}

/// AC-2's continuation fixture (Design "apt fixtures", task0002): a
/// scroll-region-based apt-like stream, held at a SINGLE dims (resize
/// coverage is AC-1's). Log lines print and scroll INSIDE a narrowed
/// scroll region (rows 1..rows-1); a progress bar redraws at the pinned
/// row (row `rows`, 1-indexed — OUTSIDE the region) via absolute
/// positioning wrapped in DECSC/DECRC — never a cursor-up short-line
/// pattern, which `vt100` and `term_core` render differently
/// (task0001's `apt_style_recording_past_a_wrapped_ring_restores_the_shadow_parsers_screen_faithfully`
/// doc comment), which would confound the reference comparison AC-2 needs.
fn synth_apt_region_stream(
    _cols: u16,
    rows: u16,
    log_start: u32,
    log_count: u32,
    bar_pcts: &[u32],
) -> Vec<u8> {
    let mut b = Vec::new();
    // Narrow the scroll region to leave the bottom row for the pinned bar.
    b.extend_from_slice(format!("\x1b[1;{}r", rows - 1).as_bytes());
    b.extend_from_slice(b"\x1b[1;1H");
    for i in log_start..log_start + log_count {
        b.extend_from_slice(format!("apt region log line {i} percent\r\n").as_bytes());
        let pct = bar_pcts[((i - log_start) as usize) % bar_pcts.len()];
        let filled = (pct as usize * 60) / 100;
        b.extend_from_slice(b"\x1b7");
        b.extend_from_slice(
            format!(
                "\x1b[{rows};1H\x1b[42m\x1b[30m[{pct:3}%] [{}{}]\x1b[49m\x1b[39m\x1b[0m",
                "\u{2588}".repeat(filled),
                " ".repeat(60usize.saturating_sub(filled)),
            )
            .as_bytes(),
        );
        b.extend_from_slice(b"\x1b8");
    }
    b
}

/// AC-2 (FR9; TS-4, task0002 D8/D3): the apt-like REGION continuation
/// fixture (Design "apt fixtures") past a wrapped ring's capacity,
/// snapshotted while its scroll region is still active (this fixture never
/// resets the region, so it stays active throughout by construction).
/// `vt100` and `term_core` agree on this fixture's rows (asserted as a
/// precondition below) — unlike the cursor-up-short-line apt fixture AC-1
/// reuses from task0001, this fixture never triggers the vt100/term_core
/// divergence task0001's own doc comment records, so a whole-stream
/// `term_core` reference is a valid oracle for the CONTINUED-OUTPUT half
/// of this test.
#[test]
fn apt_region_stream_past_a_wrapped_ring_matches_a_reference_after_continued_output() {
    use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
    use term_core::terminal_core::{MODE_ORIGIN, TerminalCore};

    let cols: u16 = 120;
    let rows: u16 = 40;

    let past_capacity = synth_apt_region_stream(cols, rows, 0, 200, &[10, 25, 50, 75, 90]);
    let continuation = synth_apt_region_stream(cols, rows, 200, 20, &[15, 40, 65, 95]);

    // Precondition (Test Notes: "vt100 and term_core must agree on the
    // fixture's rows before the snapshot"): compare a real vt100 parser
    // against a fresh term_core terminal fed the SAME bytes, before any
    // snapshot logic runs.
    let mut shadow = vt100::Parser::new(rows, cols, 0);
    shadow.process(&past_capacity);
    let mut agreement_check = TerminalCore::new(cols, rows, 10_000);
    agreement_check.process_pty_data_fully(&past_capacity);
    for r in 0..rows {
        let vt = shadow
            .screen()
            .rows(0, cols)
            .nth(r as usize)
            .unwrap_or_default();
        let tc = agreement_check.get_line_text(r);
        assert_eq!(
            tc.trim_end(),
            vt.trim_end(),
            "precondition: vt100 and term_core must agree on row {r} of the \
             region-continuation fixture"
        );
    }

    let capacity = past_capacity.len() / 3;
    let mut rb = ScrollbackRingBuffer::new(capacity);
    rb.attribute_write(cols, rows, &past_capacity);
    let (raw, segments, ring_wrapped) = rb.read_segments_with_wrap_state();
    assert!(
        ring_wrapped,
        "test prerequisite: the ring must have wrapped"
    );

    let screen_dump = shadow.screen().contents_formatted();
    let (payload, out_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &screen_dump,
        false,
        ring_wrapped,
        (cols, rows),
        10_000,
    );

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    // Feed further log lines + bar redraws to the client...
    client.process_pty_data_fully(&continuation);

    // ...and the entire stream to a fresh reference.
    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&past_capacity);
    reference.process_pty_data_fully(&continuation);

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch after continued region output"
        );
    }
    assert_eq!(
        client.get_scroll_region_top(),
        reference.get_scroll_region_top()
    );
    assert_eq!(
        client.get_scroll_region_bottom(),
        reference.get_scroll_region_bottom()
    );
    assert_eq!(
        client.get_mode(MODE_ORIGIN),
        reference.get_mode(MODE_ORIGIN)
    );
    assert_eq!(client.get_cursor_row(), reference.get_cursor_row());
    assert_eq!(client.get_cursor_col(), reference.get_cursor_col());
}

/// AC-1, AC-2 (round-7 rework, review round-6 findings
/// `bb3353636b0206cb` / `4254f3a66c1c3f5e`): replaces
/// `resize_storm_beyond_marker_cap_replays_without_cross_line_mixing`,
/// which could not fail — its fixture was too small to wrap the ring,
/// so the round-5/6 buggy implementation and the fix produced
/// IDENTICAL segment lists and replay grids, and its mixing detector
/// found nothing to detect in either case (review round-6 finding
/// `4254f3a66c1c3f5e`).
///
/// This helper:
/// 1. Puts detectable apt-style bar/log content (phase 0, the SAME
///    fixture `apt_style_recording_replays_without_cross_line_mixing`
///    uses, proven to mix when replayed under the wrong dims) BEFORE a
///    resize storm, keeping the storm's own redraws small so phase 0
///    stays on the replayed grid rather than scrolling out of view.
/// 2. Drives a REAL `ScrollbackRingBuffer` (not hand-built segments)
///    with a capacity small enough that it ACTUALLY WRAPS (content
///    bytes get evicted), so `enforce_dim_marker_cap` (count, beyond
///    `MAX_DIM_MARKERS` markers) fires against a genuinely wrapped
///    ring — sized so the storm produces EXACTLY `eviction_count`
///    evictions (the caller's choice — D2''''', review round-7 finding
///    `01f91fe698ceb287`: the round-7 test pinned eviction to exactly
///    one and could not reach the shapes that broke at 2+).
/// 3. Returns `(no_segments_mixed, full_mixed, capped_mixed,
///    capped_segments, full_segments, eviction_count)` for the caller
///    to assert on.
fn run_resize_storm_cap_eviction_case(
    eviction_count: usize,
) -> (
    usize,
    usize,
    usize,
    Vec<(usize, u16, u16)>,
    Vec<(usize, u16, u16)>,
    usize,
) {
    use crate::mux::scrollback_buffer::{MAX_DIM_MARKERS, ScrollbackRingBuffer};
    use term_core::terminal_core::TerminalCore;

    let cols: u16 = 120;
    let rows_a: u16 = 47;
    let rows_b: u16 = 48;

    let (phase0_bytes, phase0_segments) = synth_apt_bytes_with_midrun_resize(cols, rows_a, rows_b);
    assert_eq!(phase0_segments.len(), 2, "test prerequisite");

    // Storm: enough additional resize steps that the TOTAL marker count
    // (phase 0's 2 + the storm's) exceeds MAX_DIM_MARKERS by exactly
    // `eviction_count`. Each step's redraw is a small bar/log fragment
    // (not a full apt cycle) so phase 0 stays within the final
    // viewport regardless of how large the storm gets.
    let storm_steps = MAX_DIM_MARKERS + eviction_count - phase0_segments.len();
    let storm_rows = [rows_a, rows_b];
    let mut storm_chunks: Vec<(u16, u16, Vec<u8>)> = Vec::with_capacity(storm_steps);
    for step in 0..storm_steps {
        let rows = storm_rows[step % 2];
        let redraw = if step % 2 == 0 {
            format!("\x1b7\x1b[{rows};0f[{}\u{2588}]\x1b8", step % 10).into_bytes()
        } else {
            format!("storm log at step {step} percent\r\n").into_bytes()
        };
        storm_chunks.push((cols, rows, redraw));
    }
    // The replay target is whatever dims the pane is ACTUALLY at by the
    // time this snapshot is assembled — the storm's OWN final step's
    // dims (mirroring the real end-to-end scenario: a client's replay
    // target is its current window size, which drives the daemon's
    // most-recently-recorded resize). Earlier storm-length-agnostic
    // versions of this test hardcoded `target_rows = rows_a`, which only
    // coincidentally matched the single-eviction storm's own parity;
    // generalizing `storm_steps` across eviction counts requires
    // deriving it instead of assuming it.
    let target_rows: u16 = storm_chunks
        .last()
        .map(|&(_, rows, _)| rows)
        .unwrap_or(rows_a);

    // Ring capacity: small enough that the total content (phase 0 +
    // storm) overruns it by a modest margin, so the ring ACTUALLY
    // WRAPS (AC-2) — evicting a slice of phase 0's own leading filler
    // text (plain unrelated history, not part of the mixing
    // detector's pattern), while everything the detector cares about
    // survives.
    let storm_len: usize = storm_chunks.iter().map(|(_, _, c)| c.len()).sum();
    let total_len = phase0_bytes.len() + storm_len;
    let margin = 500usize;
    assert!(
        total_len > margin * 2,
        "test prerequisite: fixture must be large enough to leave a \
         wrap margin"
    );
    let capacity = total_len - margin;

    let mut rb = ScrollbackRingBuffer::new(capacity);
    let mut full_markers: Vec<(u64, u16, u16)> = Vec::new();
    let mut total_written: u64 = 0;

    // Replay phase 0 through the REAL ring, chunked at ITS OWN
    // recorded segment boundaries, so `write_resize_marker` records
    // the same offsets `synth_apt_bytes_with_midrun_resize` assumes.
    for (i, seg) in phase0_segments.iter().enumerate() {
        let end = phase0_segments
            .get(i + 1)
            .map(|s| s.offset as usize)
            .unwrap_or(phase0_bytes.len());
        rb.write_resize_marker(seg.cols, seg.rows);
        full_markers.push((total_written, seg.cols, seg.rows));
        let chunk = &phase0_bytes[seg.offset as usize..end];
        rb.write(chunk);
        total_written += chunk.len() as u64;
    }
    for (step_cols, step_rows, chunk) in &storm_chunks {
        rb.write_resize_marker(*step_cols, *step_rows);
        full_markers.push((total_written, *step_cols, *step_rows));
        rb.write(chunk);
        total_written += chunk.len() as u64;
    }
    assert_eq!(
        full_markers.len(),
        MAX_DIM_MARKERS + eviction_count,
        "test prerequisite: total recorded markers must exceed \
         MAX_DIM_MARKERS by exactly {eviction_count}, so the cap evicts \
         exactly that many entries"
    );

    let (raw, capped_segments) = rb.read_segments();
    assert!(
        raw.len() < total_len,
        "test prerequisite: the ring must actually have evicted some \
         content (AC-2 — the ring wraps during this test), got \
         raw.len()={} of total_len={total_len}",
        raw.len(),
    );

    let oldest_offset = total_written.saturating_sub(capacity as u64);
    let full_segments = full_attribution_segments(&full_markers, oldest_offset, raw.len());

    let replay = |segments: &[(usize, u16, u16)]| -> usize {
        let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
            &raw,
            segments,
            b"",
            false,
            (cols, target_rows),
        );
        let mut core = TerminalCore::new(cols, target_rows, 10_000);
        core.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));
        count_mixed_rows(&core, target_rows)
    };

    let no_segments_mixed = replay(&[]);
    let full_mixed = replay(&full_segments);
    let capped_mixed = replay(&capped_segments);

    (
        no_segments_mixed,
        full_mixed,
        capped_mixed,
        capped_segments,
        full_segments,
        eviction_count,
    )
}

/// AC-1, AC-2 (round-8 rework, review round-7 finding
/// `01f91fe698ceb287`, D2'''''): parameterized over eviction counts of
/// ONE, SEVERAL, and MANY — round 7's version of this test pinned
/// eviction to exactly one, so it structurally could not reach the
/// shapes where round 7's fix broke down (2+ evictions), and its
/// `capped_segments == full_segments` assertion made the mixed-row
/// comparison vacuously true (both sides identical by construction).
///
/// At EVERY eviction count this asserts the hard gate D2''''' calls
/// for: the capped replay is never worse than replaying with no
/// segments at all. At exactly one eviction, D1''''' additionally
/// recovers full uncapped attribution EXACTLY (proven: the single
/// evicted entry's own dims are known precisely via
/// `capped_head_dims`), so the stronger structural/mixed-row equality
/// is asserted there too.
///
/// For 2+ evictions the STRUCTURAL assertion (no head segment
/// synthesized — `capped_segments.len() == MAX_DIM_MARKERS`) is the
/// discriminator, not a mixed-row bound against full attribution: this
/// implementer's own measurement (using this exact fixture, parameterized
/// over eviction counts 1/4/28 as D2''''' specifies) found a
/// counter-example to "never worse than full attribution" once the
/// evicted span covers MULTIPLE distinct dimension regimes with
/// detector-relevant content in more than one of them (here: phase 0's
/// OWN two apt-cycle regions, both evicted once eviction_count >= 2,
/// since they are chronologically the OLDEST markers) — no SINGLE
/// gap-dims choice (last-evicted, as round 7 used; or none, as
/// D1''''' uses) can correctly render two genuinely-different regimes
/// under one dims value. At `MAX_DIM_MARKERS == 24` (round-8):
/// eviction_count=4 gave 1 mixed row, eviction_count=28 gave 13 (both
/// `> full_mixed == 0` but `<= no_segments_mixed`). Re-measured at
/// `MAX_DIM_MARKERS == 62` (round-9, D1''''''): eviction_count=4 still
/// gives 1, eviction_count=28 now gives 12 — a small numeric shift
/// from the longer storm this fixture now needs to reach the same
/// eviction count, not a change in kind. Reverting D1''''' to round-7's
/// `capped_head_dims`-for-any-eviction-count behavior on this SAME
/// fixture reproduces the IDENTICAL (per-cap) mixed-row counts (round
/// 7's single-dims guess is no better here) — this is a genuine,
/// measured residual documented as an accepted precision loss (mirrors
/// `MAX_DIM_MARKERS`'s own doc), not a regression D1''''' introduces,
/// AND NOT one D1'''''' (raising the cap) closes: it persists at any
/// cap value once a storm forces 2+ evictions, per this same
/// implementer's re-measurement at 62. `VERIFICATION.md`'s FR2
/// coverage reflects this (AC-10).
///
/// Confirmed to fail pre-fix: reverting D1''''' (restoring round-7's
/// unconditional `capped_head_dims` fallback for ANY eviction count)
/// makes the eviction-count-4 and eviction-count-28 cases' structural
/// assertion fail (`capped_segments.len()` comes out as
/// `MAX_DIM_MARKERS + 1`, with an extra head segment at position 0,
/// instead of `MAX_DIM_MARKERS`).
#[test]
fn resize_storm_beyond_marker_cap_replays_no_worse_than_full_attribution() {
    use crate::mux::scrollback_buffer::MAX_DIM_MARKERS;

    // AC-1 (round-9 rework, D1''''''): `eviction_count = 0` is a storm
    // whose total recorded markers land EXACTLY at the new cap (62) —
    // "a resize storm of any length up to the wire ceiling" — added
    // alongside the raised cap to pin the boundary case the round-8
    // sweep's "marker 26/32/52 vs cap 62" measurement relies on: no
    // eviction ever happens, so `read_segments` returns every marker
    // and `capped_segments` is IDENTICAL to `full_segments`, not merely
    // "no worse".
    for eviction_count in [0usize, 1, 4, 28] {
        let (no_segments_mixed, full_mixed, capped_mixed, capped_segments, full_segments, _) =
            run_resize_storm_cap_eviction_case(eviction_count);

        // Discrimination premise (hard gate, D2'''''): this fixture must
        // be ABLE to show a difference at every eviction count, or the
        // assertions below are vacuous.
        assert!(
            no_segments_mixed > full_mixed,
            "eviction_count={eviction_count}: fixture is not \
             discriminating: no-segment replay ({no_segments_mixed} \
             mixed rows) must mix MORE than full-attribution replay \
             ({full_mixed} mixed rows) — if this ever stops holding, \
             rebuild the fixture rather than weaken this assertion"
        );

        // AC-1 hard gate (always provable, D2'''''): the capped replay
        // must never be WORSE than replaying with no segments at all.
        assert!(
            capped_mixed <= no_segments_mixed,
            "eviction_count={eviction_count}: capped replay \
             ({capped_mixed} mixed rows) is WORSE than replaying with \
             no segments at all ({no_segments_mixed} mixed rows) — the \
             cap-eviction fallback must never be worse than shipping \
             nothing"
        );

        if eviction_count == 0 {
            // AC-1 (round-9 rework, D1''''''): a storm landing EXACTLY
            // at the cap needs no eviction at all — `read_segments`
            // returns every recorded marker, so the capped segment list
            // is not merely "no worse" but IDENTICAL to full uncapped
            // attribution, byte for byte.
            assert_eq!(
                capped_segments.len(),
                MAX_DIM_MARKERS,
                "with zero evictions, every recorded marker survives — \
                 no head segment is synthesized because none is needed"
            );
            assert_eq!(
                capped_segments, full_segments,
                "with zero cap evictions, the capped segment list must \
                 be IDENTICAL to full uncapped attribution"
            );
            assert_eq!(
                capped_mixed, full_mixed,
                "with zero cap evictions, capped replay must mix \
                 EXACTLY as many rows as full uncapped attribution"
            );
        } else if eviction_count == 1 {
            // D1''''' guarantees EXACT recovery of full attribution for
            // a single eviction (capped_head_dims names exactly the one
            // entry that was evicted) — the strongest possible check.
            assert_eq!(
                capped_segments.len(),
                MAX_DIM_MARKERS + 1,
                "with exactly one eviction, the single evicted entry's \
                 gap still becomes its own head segment"
            );
            assert_eq!(
                capped_segments, full_segments,
                "with exactly one cap eviction, the capped segment list \
                 must be IDENTICAL to full uncapped attribution"
            );
            assert!(capped_mixed <= full_mixed);
        } else {
            // D1''''': with 2+ evictions, no head segment is
            // synthesized at all — only the MAX_DIM_MARKERS survivors
            // appear, each at its own true position. This is the
            // discriminator against reverting the fix (see doc above);
            // it is NOT asserted that `capped_mixed <= full_mixed` here
            // — see this test's doc comment for the measured
            // counter-example, which round-9's cap raise does not
            // close (only how long a storm must be to reach it).
            assert_eq!(
                capped_segments.len(),
                MAX_DIM_MARKERS,
                "with {eviction_count} evictions, no head segment \
                 should be synthesized for the gap"
            );
        }
    }
}

/// Cursor-addressed TUI-style recording: a status/input row pinned to
/// the bottom of the screen via a scroll region excluding it (mirrors
/// an app like Claude Code keeping its status/input line in place while
/// chat content scrolls above — `project_status_bar_design`), re-painted
/// in place via `ESC7 CUP <text> ESC8` on every tick while chat content
/// scrolls via plain `\r\n` above it. Row count changes mid-run; the
/// caller supplies the segment boundaries explicitly (no marker bytes).
///
/// Distinguishes from AC-2's apt-style bar (bottom-row-only redraw, no
/// scrolling *inside* the reserved region) via genuinely scrolling chat
/// content interleaved with the fixed-row status — the mechanism PROBE D
/// identified (a scroll-region boundary computed for the WRONG row
/// count lets scrolled content invade a row that should have been
/// exempt) reproducing for "an app with a pinned status line" shape, not
/// only "an app with a growing progress bar" shape.
fn synth_tui_cursor_addressed_bytes_with_midrun_resize(
    rows_a: u16,
    rows_b: u16,
) -> (Vec<u8>, Vec<ReplaySegment>) {
    let mut b = Vec::new();
    let mut segments = vec![ReplaySegment {
        offset: 0,
        cols: 100,
        rows: rows_a,
    }];
    for i in 0..rows_a.max(rows_b) + 20 {
        b.extend_from_slice(format!("chat history line {i}\r\n").as_bytes());
    }
    // Phase A: reserve the bottom row for a status/input line (scroll
    // region excludes it) while chat content scrolls above and the
    // status row is periodically re-painted in place.
    b.extend_from_slice(b"\n\x1b7");
    b.extend_from_slice(format!("\x1b[0;{}r", rows_a - 1).as_bytes());
    b.extend_from_slice(b"\x1b8\x1b[1A");
    for tick in 0..3u32 {
        b.extend_from_slice(format!("chat reply A line {tick}\r\n").as_bytes());
        b.extend_from_slice(format!("\x1b7\x1b[{rows_a};0fSTATUS-A[{tick}]\x1b8").as_bytes());
    }
    // SIGWINCH: a new segment at this offset, then the same pattern
    // re-established for rows_b.
    segments.push(ReplaySegment {
        offset: b.len() as u32,
        cols: 100,
        rows: rows_b,
    });
    b.extend_from_slice(b"\n\x1b7");
    b.extend_from_slice(format!("\x1b[0;{}r", rows_b - 1).as_bytes());
    b.extend_from_slice(b"\x1b8\x1b[1A");
    for tick in 0..3u32 {
        b.extend_from_slice(format!("chat reply B line {tick}\r\n").as_bytes());
        b.extend_from_slice(format!("\x1b7\x1b[{rows_b};0fSTATUS-B[{tick}]\x1b8").as_bytes());
    }
    (b, segments)
}

/// AC-2: the TUI-style cursor-addressed recording, replayed after the
/// fix, shows no row mixing content from phase A (pre-resize) and phase
/// B (post-resize) — the reported symptom's shape (Claude Code's
/// status/input area), covering a pinned-status-row app rather than
/// only apt's growing-bar pattern. Fails when segment attribution is
/// dropped, for every combination (each pairs a row-count change with a
/// replay target that differs from at least one recorded size).
#[test]
fn tui_cursor_addressed_recording_replays_without_cross_line_mixing() {
    use term_core::terminal_core::TerminalCore;
    let cols: u16 = 100;
    for (rec_a, rec_b, replay_rows) in [
        (30u16, 32u16, 30u16),
        (30, 32, 32),
        (32, 30, 30),
        (32, 30, 32),
    ] {
        let (recording, segments) =
            synth_tui_cursor_addressed_bytes_with_midrun_resize(rec_a, rec_b);
        let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
            &recording,
            &to_tuples(&segments),
            b"",
            false,
            (cols, replay_rows),
        );
        let mut core = TerminalCore::new(cols, replay_rows, 10_000);
        core.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));
        let mut tainted = Vec::new();
        for r in 0..replay_rows {
            let line = core.get_line_text(r);
            // A row is tainted if it shows a STATUS redraw fragment
            // glued to a scrolled chat-line fragment — the "bar
            // fragment landed on a log-content row" shape (mirrors
            // AC-2's `has_bar && has_log` detector; not phase-specific,
            // since the coordinate-drift bug glues ANY two logically
            // distinct writes onto one physical row).
            if line.contains("STATUS-") && line.contains(" line ") {
                tainted.push(format!("row {r}: {line}"));
            }
        }
        assert!(
            tainted.is_empty(),
            "rec {rec_a}->{rec_b} replay@{replay_rows}: expected zero cross-phase-mixed rows, \
             got {tainted:?}"
        );
    }
}

/// AC-11: a segment-free recording replays to the SAME grid as a
/// straight `reset()` + full-drain `process_pty_data_fully` — the
/// documented older-daemon degradation (single-dimension replay).
#[test]
fn segment_free_recording_replays_unchanged() {
    use term_core::terminal_core::TerminalCore;
    let cols: u16 = 80;
    let rows: u16 = 24;
    let mut recording = Vec::new();
    for i in 0..40 {
        recording.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    recording.extend_from_slice(b"\x1b[31mred\x1b[0m plain\r\n");

    let mut reference = TerminalCore::new(cols, rows, 1000);
    reference.reset();
    reference.process_pty_data_fully(&recording);

    let mut under_test = TerminalCore::new(cols, rows, 1000);
    under_test.reset_and_replay_segments(&recording, &[]);

    for r in 0..rows {
        assert_eq!(
            under_test.get_line_text(r),
            reference.get_line_text(r),
            "row {r} differs: segment-free replay must be byte-path unchanged"
        );
    }
    assert_eq!(under_test.cols(), reference.cols());
    assert_eq!(under_test.rows(), reference.rows());
}

// ── AC-1: marker-SHAPED byte sequences carry no segment authority,
// however they are shaped, split, nested, or reconstructed ───────────

/// Byte-for-byte the OLD (pre-round-4) marker wire format. Kept ONLY as
/// adversarial test fixture data — no production decoder recognizes
/// this shape any more.
fn legacy_marker_shaped_bytes(cols: u16, rows: u16) -> Vec<u8> {
    format!("\x1b]777;emterm;resize;{cols};{rows}\x07").into_bytes()
}

/// Replay `bytes` with NO segments and return the reflow delta —
/// the shared witness every AC-1 scenario below checks: per the task's
/// test notes, `core.cols()`/`rows()` after a replay always equal the
/// caller's target regardless of what happened mid-drain, so only the
/// reflow counter (or a grid fingerprint) actually distinguishes
/// "the adversarial bytes were honored" from "they were inert".
fn reflow_delta_replaying_with_no_segments(bytes: &[u8]) -> u64 {
    use term_core::terminal_core::TerminalCore;
    let mut core = TerminalCore::new(80, 24, 1000);
    let before = core.reflow_call_count();
    core.reset_and_replay_segments(bytes, &[]);
    core.reflow_call_count() - before
}

/// AC-1 scenario 1 (bare sequence): a marker-shaped OSC 777 body
/// arriving as ordinary child PTY output in ONE read now reaches the
/// scrollback ring UNSTRIPPED (task0004 round-4 rework: there is no
/// more `resize`-kind special-casing at the write path — see
/// `strip_pty_output_for_scrollback_write`'s doc comment) — and that is
/// fine: the ring's `write_resize_marker` was never called for it, so
/// `read_segments()` reports zero segments regardless of what the byte
/// content looks like, and replay never resizes on it.
#[test]
fn ac1_bare_marker_shaped_bytes_reach_ring_but_carry_no_segment_authority() {
    let scrollback = new_scrollback(4096);
    let marker = legacy_marker_shaped_bytes(65535, 65535);
    let mut chunk = b"before".to_vec();
    chunk.extend_from_slice(&marker);
    chunk.extend_from_slice(b"after");
    let filter = feed_all(&scrollback, &[&chunk]);
    assert_eq!(filter.pending_len(), 0);
    let (bytes, segments) = scrollback.lock().unwrap().read_segments();
    assert!(
        segments.is_empty(),
        "a ring that never called write_resize_marker must report zero \
         segments, regardless of marker-shaped byte content"
    );
    assert!(
        bytes.windows(marker.len()).any(|w| w == marker.as_slice()),
        "marker-shaped bytes now survive as ordinary (authority-less) \
         content — there is no more resize-kind stripping"
    );
    assert_eq!(
        reflow_delta_replaying_with_no_segments(&bytes),
        0,
        "surviving marker-shaped bytes must never trigger a resize"
    );
}

/// AC-1 scenario 2 (split across two filter batches): the SAME
/// marker-shaped sequence, but delivered across TWO separate
/// `ScrollbackWriteFilter::feed` calls (mirroring two PTY `read()`
/// calls) — still zero segment authority, zero reflow.
///
/// Confirmed to fail pre-fix: against the removed round-1/round-2
/// write-path strip (`strip_pty_output_for_scrollback_write`'s old
/// `resize`-kind special-casing), a marker split exactly at this point
/// was the round-3 finding `4a22bd439fcdaf56` scenario — the flush
/// boundary let a complete marker re-form on the ring side across the
/// two batches, undetected by either single-batch strip pass.
#[test]
fn ac1_marker_shaped_bytes_split_across_two_filter_batches_carry_no_segment_authority() {
    let scrollback = new_scrollback(4096);
    let marker = legacy_marker_shaped_bytes(4096, 4096);
    let mut full = b"before".to_vec();
    full.extend_from_slice(&marker);
    full.extend_from_slice(b"after");
    let split_at = b"before".len() + marker.len() / 2;
    let (first, second) = full.split_at(split_at);
    let filter = feed_all(&scrollback, &[first, second]);
    assert_eq!(filter.pending_len(), 0);
    let (bytes, segments) = scrollback.lock().unwrap().read_segments();
    assert!(segments.is_empty());
    assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
}

/// AC-1 scenario 3 (split at a lone-trailing-ESC boundary / pending
/// overflow escape hatch): a child process opens an unterminated DCS
/// introducer (`ESC P`, never closed) and keeps writing until
/// `ScrollbackWriteFilter`'s pending buffer exceeds
/// [`SCROLLBACK_FILTER_PENDING_CAP`], with a well-formed marker-shaped
/// sequence sitting in the padding that gets flushed raw by the
/// overflow escape hatch. Zero segment authority, zero reflow.
#[test]
fn ac1_marker_shaped_bytes_via_pending_overflow_escape_hatch_carry_no_segment_authority() {
    let scrollback = new_scrollback(4096);
    let intro = b"\x1bPtmux;".to_vec();
    let mut padding_and_marker: Vec<u8> = std::iter::repeat_n(b'A', 520 * 1024).collect();
    padding_and_marker.extend_from_slice(&legacy_marker_shaped_bytes(999, 999));

    let mut filter = ScrollbackWriteFilter::new();
    let (_dims, first) = filter.feed(&intro, (80, 24));
    assert!(
        first.is_empty(),
        "introducer alone must still be held pending"
    );
    let (_dims, flushed) = filter.feed(&padding_and_marker, (80, 24));
    assert!(
        !flushed.is_empty(),
        "the overflow escape hatch must have fired for this test to be meaningful"
    );
    assert_eq!(filter.pending_len(), 0);
    scrollback.lock().unwrap().write(&flushed);
    let (bytes, segments) = scrollback.lock().unwrap().read_segments();
    assert!(segments.is_empty());
    assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
}

/// AC-1 scenario 4 (nested in a non-SIXEL DCS): mirrors
/// `printf '\ePtmux;\e\e]777;emterm;resize;999;999\a\e\\'` — a doubled
/// ESC is the tmux DCS passthrough convention for escaping a literal
/// ESC inside the passthrough body, so the marker-shaped `ESC ]` is
/// nested inside a non-SIXEL DCS. Zero segment authority, zero reflow.
#[test]
fn ac1_marker_shaped_bytes_nested_in_non_sixel_dcs_carry_no_segment_authority() {
    let scrollback = new_scrollback(4096);
    let mut chunk = b"before".to_vec();
    chunk.extend_from_slice(b"\x1bPtmux;\x1b");
    chunk.extend_from_slice(&legacy_marker_shaped_bytes(999, 999));
    chunk.extend_from_slice(b"\x1b\\");
    chunk.extend_from_slice(b"after");

    let filter = feed_all(&scrollback, &[&chunk]);
    assert_eq!(filter.pending_len(), 0);
    let (bytes, segments) = scrollback.lock().unwrap().read_segments();
    assert!(segments.is_empty());
    assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
}

/// AC-1 scenario 5 (formed by concatenation after a strip pass): round-3
/// finding `95fb7c115b0b64da`'s exact adversarial construction —
/// `P + P + SIXEL_DCS + "1;1\a" + "999;999\a"` (`P` = the marker prefix,
/// unterminated) — which, under the OLD single-pass literal strip
/// (`strip_literal_resize_marker_occurrences`, removed by D1'),
/// concatenated a surviving prefix with a surviving body into a
/// complete forged marker. Feeds `strip_pty_output_for_scrollback_write`
/// directly (the exact function the finding targeted) — since D1'
/// carries dimensions structurally (never as bytes), there is no
/// literal-marker strip pass left to defeat, and whatever this input
/// reduces to can never carry segment authority regardless.
#[test]
fn ac1_marker_shaped_bytes_formed_by_concatenation_after_strip_carry_no_segment_authority() {
    let scrollback = new_scrollback(4096);
    let prefix = b"\x1b]777;emterm;resize;"; // unterminated marker prefix ("P")
    let sixel = b"\x1bP1;0;0q\"1;1;5;5#0;2;0;0;0\x1b\\"; // complete SIXEL DCS (stripped)
    let mut chunk = Vec::new();
    chunk.extend_from_slice(prefix);
    chunk.extend_from_slice(prefix);
    chunk.extend_from_slice(sixel);
    chunk.extend_from_slice(b"1;1\x07");
    chunk.extend_from_slice(b"999;999\x07");

    let stripped = strip_pty_output_for_scrollback_write(&chunk);
    scrollback.lock().unwrap().write(&stripped);
    let (bytes, segments) = scrollback.lock().unwrap().read_segments();
    assert!(
        segments.is_empty(),
        "no write_resize_marker call was ever made for this ring"
    );
    assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
}

// ── round-trip fidelity: replayed grid matches a live-fed reference ───

/// Round-trip fingerprint equality. The grid (line text + cursor) of a
/// core fed a recording LIVE (`process_pty_data_fully`, resizing
/// directly at each real PTY resize — mirrors what the daemon's own
/// shadow parser sees) must equal that of a core replayed from the
/// SNAPSHOT-ASSEMBLED bytes (`build_snapshot_bytes` ->
/// `reset_and_replay_segments`, segment-aware), for both a
/// segment-free recording and a resize-spanning one. This is the
/// SPEC.md unit test list's "switch-time grid == replay grid"
/// round-trip check: unlike the cross-line-mixing detectors above, it
/// also catches a regression that drops, duplicates, or blanks content
/// without literally interleaving two fragments onto one row.
#[test]
fn round_trip_grid_fingerprint_matches_live_feed_for_resize_free_and_resize_spanning() {
    use term_core::terminal_core::TerminalCore;
    let cols: u16 = 80;
    let rows: u16 = 24;

    fn fingerprint(core: &TerminalCore) -> (Vec<String>, u16, u16) {
        let mut lines = Vec::with_capacity(core.rows() as usize);
        for r in 0..core.rows() {
            lines.push(core.get_line_text(r));
        }
        (lines, core.get_cursor_col(), core.get_cursor_row())
    }

    // Case 1: segment-free recording.
    {
        let mut recording = Vec::new();
        for i in 0..30 {
            recording.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        let mut live = TerminalCore::new(cols, rows, 1000);
        live.process_pty_data_fully(&recording);

        let (snap, snap_segments) =
            crate::mux::snapshot_bytes::build_snapshot_bytes(&recording, &[], b"", false, (80, 24));
        let mut replayed = TerminalCore::new(cols, rows, 1000);
        replayed.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));

        assert_eq!(
            fingerprint(&live),
            fingerprint(&replayed),
            "segment-free round-trip must match the live-fed reference"
        );
    }

    // Case 2: resize-spanning recording. The live reference resizes
    // directly at the exact point in the stream the segment boundary
    // represents, then restores to the target size at the end —
    // mirroring exactly what `reset_and_replay_segments` does.
    {
        let before = b"before\r\n".to_vec();
        let after = b"after\r\n".to_vec();
        let mut recording = before.clone();
        recording.extend_from_slice(&after);
        let segments = [
            ReplaySegment {
                offset: 0,
                cols,
                rows,
            },
            ReplaySegment {
                offset: before.len() as u32,
                cols: 100,
                rows: 30,
            },
        ];

        let mut live = TerminalCore::new(cols, rows, 1000);
        live.process_pty_data_fully(b"before\r\n");
        live.resize(100, 30);
        live.process_pty_data_fully(b"after\r\n");
        live.resize(cols, rows);

        let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
            &recording,
            &to_tuples(&segments),
            b"",
            false,
            (cols, rows),
        );
        let mut replayed = TerminalCore::new(cols, rows, 1000);
        replayed.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));

        assert_eq!(
            fingerprint(&live),
            fingerprint(&replayed),
            "resize-spanning round-trip must match the live-fed reference"
        );
    }
}

/// Builds ONE phase's raw content (no marker bytes) for the
/// differing-DIMENSIONS scenario below — reused both to assemble the
/// segment-bearing recording AND to drive the segment-free LIVE
/// reference (which resizes directly instead of via a segment).
///
/// `narrower_cols` is fixed across both phases (the narrower of the
/// recording's two cols values) so the long line wraps whenever the
/// narrower width is in effect, regardless of which phase is active.
fn synth_dims_phase_bytes(rows: u16, narrower_cols: u16, label: &str) -> Vec<u8> {
    let mut b = Vec::new();
    for i in 0..rows + 20 {
        b.extend_from_slice(format!("chat history line {i}\r\n").as_bytes());
    }
    b.extend_from_slice(b"\n\x1b7");
    b.extend_from_slice(format!("\x1b[0;{}r", rows.saturating_sub(1)).as_bytes());
    b.extend_from_slice(b"\x1b8\x1b[1A");
    // Longer than the narrower width in play across the whole
    // recording, so it wraps whenever that width is in effect.
    let long_line = "L".repeat(narrower_cols as usize + 20);
    for tick in 0..3u32 {
        b.extend_from_slice(format!("chat reply {label} {tick} {long_line}\r\n").as_bytes());
        b.extend_from_slice(format!("\x1b7\x1b[{rows};0f{label}-STATUS[{tick}]\x1b8").as_bytes());
    }
    b
}

/// The PREVIOUS version of this test (task0002 era) could never fail
/// regardless of correctness — its rows were identical across both
/// phases (so PROBE D's row-count coordinate-drift mechanism, which
/// needs a ROW COUNT change to misinterpret DECSTBM / CUP coordinates,
/// never fired at all), and its taint detector searched for a substring
/// the cols-varying synthesized content never actually contained. This
/// version varies BOTH rows and cols across the segment boundary
/// (actually exercises the drift mechanism), still includes a line
/// longer than the narrower width so it wraps, and compares against a
/// LIVE-FED reference grid FINGERPRINT — confirmed, while developing
/// this test, to FAIL when segment attribution is dropped (empty
/// segments passed to `reset_and_replay_segments`).
#[test]
fn differing_dimensions_recording_matches_live_feed_fingerprint() {
    use term_core::terminal_core::TerminalCore;

    fn fingerprint(core: &TerminalCore) -> (Vec<String>, u16, u16) {
        let mut lines = Vec::with_capacity(core.rows() as usize);
        for r in 0..core.rows() {
            lines.push(core.get_line_text(r));
        }
        (lines, core.get_cursor_col(), core.get_cursor_row())
    }

    for ((cols_a, rows_a), (cols_b, rows_b), (replay_cols, replay_rows)) in [
        ((100u16, 32u16), (40u16, 24u16), (100u16, 32u16)),
        ((100, 32), (40, 24), (40, 24)),
        ((40, 24), (100, 32), (100, 32)),
        ((40, 24), (100, 32), (40, 24)),
    ] {
        let narrower_cols = cols_a.min(cols_b);
        let phase_a = synth_dims_phase_bytes(rows_a, narrower_cols, "A");
        let phase_b = synth_dims_phase_bytes(rows_b, narrower_cols, "B");

        let mut recording = phase_a.clone();
        recording.extend_from_slice(&phase_b);
        let segments = [
            ReplaySegment {
                offset: 0,
                cols: cols_a,
                rows: rows_a,
            },
            ReplaySegment {
                offset: phase_a.len() as u32,
                cols: cols_b,
                rows: rows_b,
            },
        ];

        // Live reference: a live parser resizes directly at the exact
        // points in the stream the segments represent, mirroring
        // exactly what `reset_and_replay_segments` does.
        let mut live = TerminalCore::new(replay_cols, replay_rows, 10_000);
        live.resize(cols_a, rows_a);
        live.process_pty_data_fully(&phase_a);
        live.resize(cols_b, rows_b);
        live.process_pty_data_fully(&phase_b);
        live.resize(replay_cols, replay_rows);

        let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
            &recording,
            &to_tuples(&segments),
            b"",
            false,
            (replay_cols, replay_rows),
        );
        let mut replayed = TerminalCore::new(replay_cols, replay_rows, 10_000);
        replayed.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));

        assert_eq!(
            fingerprint(&live),
            fingerprint(&replayed),
            "dims ({cols_a},{rows_a})->({cols_b},{rows_b}) replay@({replay_cols},{replay_rows}): \
             replayed grid must match the live-fed reference"
        );
    }
}

// ── G1 self-deadlock regression (mux-window-switch-output-hang
// task0004, review round 3 finding `22251d51cc98261e`) ────────────────

/// AC-1: the EOF branch of `pty_reader_loop` must release
/// `output_target` BEFORE its `blocking_send`, exactly like the data
/// path a few dozen lines below it already does (that path's own
/// comment: "IMPORTANT: release lock before blocking_send to avoid
/// deadlock"). Pre-fix, the EOF branch held the guard for the entire
/// `blocking_send` call; while parked there (channel saturated), ANY
/// other thread taking the SAME `output_target` mutex synchronously —
/// exactly what the connection task's `resume_pane_with_permit`
/// (`pane.rs`) does, reachable via task0003's fair-permit path — would
/// block on `lock()` until the send completed, which itself could only
/// complete once the connection task's own drain arm freed capacity,
/// which it cannot do while blocked on that `lock()`. A cross-thread
/// self-deadlock of the connection.
///
/// This test simulates exactly that shape without a real connection
/// task: a reader thread hits `Ok(0)` (EOF) against a channel that
/// already holds one item (capacity 1, so the EOF empty-chunk send
/// parks), while a second thread stands in for the connection task by
/// taking the same `output_target` lock. Pre-fix, that second thread's
/// `lock()` blocks for as long as the reader stays parked (this test
/// bounds the wait so a regression fails with a clear message instead
/// of hanging the suite); post-fix, the lock is free almost
/// immediately because the EOF branch dropped its guard before ever
/// calling `blocking_send`.
#[test]
fn eof_branch_does_not_hold_output_target_across_blocking_send() {
    use std::time::Duration;

    struct ImmediateEof;
    impl Read for ImmediateEof {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    // Capacity-1 channel, already holding one item, so it is
    // saturated: the reader's EOF `blocking_send` below parks until
    // something drains it.
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);
    tx.try_send(PtyOutputChunk::pty_output(1, b"filler".to_vec()))
        .expect("channel must accept the filler chunk");

    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));

    let reader_thread = std::thread::spawn({
        let target = target.clone();
        let shadow_parser = pane.shadow_parser.clone();
        let cwd = pane.cwd.clone();
        let title = pane.title.clone();
        let title_sender = pane.title_sender.clone();
        let notification_sender = pane.notification_sender.clone();
        let agent_status_report_sender = pane.agent_status_report_sender.clone();
        let raw_passthrough = pane.raw_passthrough.clone();
        let passthrough_scanner = pane.passthrough_scanner.clone();
        let scrollback = pane.scrollback.clone();
        let dims = pane.dims.clone();
        let output_capture = pane.output_capture.clone();
        move || {
            pty_reader_loop(
                1,
                Box::new(ImmediateEof),
                target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                pane_exit_sender,
                output_capture,
            );
        }
    });

    // Give the reader thread time to reach `Ok(0)` and enter the EOF
    // branch's `blocking_send` (parked — the channel is saturated).
    std::thread::sleep(Duration::from_millis(100));

    // Stand in for the connection task's own lock acquisition
    // (`resume_pane_with_permit`, `pane.rs`, takes this same mutex
    // synchronously, no `.await` yield point). Bounded polling instead
    // of a bare `.join()` so a regression fails this ONE test with a
    // clear message rather than hanging the whole suite.
    let lock_probe = std::thread::spawn({
        let target = target.clone();
        move || {
            let _guard = target.lock().unwrap();
        }
    });
    let start = std::time::Instant::now();
    let acquired_promptly = loop {
        if lock_probe.is_finished() {
            break true;
        }
        if start.elapsed() > Duration::from_secs(1) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        acquired_promptly,
        "a connection-task-style lock() on output_target must not block on the \
         reader thread's parked EOF blocking_send (G1 self-deadlock)"
    );
    lock_probe.join().unwrap();

    // Drain the channel so the reader's parked blocking_send can
    // complete and the reader thread exits cleanly.
    let filler = rx.try_recv().expect("filler chunk must be present");
    assert_eq!(filler.pane_id, 1);
    let eof_signal = rx.blocking_recv().expect("EOF empty chunk must be sent");
    assert!(
        eof_signal.data.is_empty(),
        "EOF signal chunk must carry empty data"
    );
    reader_thread.join().unwrap();
}

// ── OSC 7 cwd detection (relocated from the deleted mux status-bar
//    engine module; mux-status-bar-removal task0001, IMPLEMENTATION.md
//    D3, AC-5). Behavior and assertions are unchanged from before the
//    move. ──────────────────────────────────────────────────────────

#[test]
fn test_detect_osc7_with_esc_st() {
    let data = b"\x1b]7;file://myhost/home/user\x1b\\";
    assert_eq!(detect_osc7_cwd(data), Some("/home/user".to_string()));
}

#[test]
fn test_detect_osc7_with_bel_st() {
    let data = b"\x1b]7;file://myhost/home/user\x07";
    assert_eq!(detect_osc7_cwd(data), Some("/home/user".to_string()));
}

#[test]
fn test_detect_osc7_embedded_in_data() {
    let data = b"some output\x1b]7;file://host/tmp\x1b\\more data";
    assert_eq!(detect_osc7_cwd(data), Some("/tmp".to_string()));
}

#[test]
fn test_detect_osc7_no_pattern() {
    let data = b"normal pty output without osc7";
    assert_eq!(detect_osc7_cwd(data), None);
}

#[test]
fn test_detect_osc7_no_st() {
    let data = b"\x1b]7;file://host/home/user";
    assert_eq!(detect_osc7_cwd(data), None);
}

#[test]
fn test_detect_osc7_url_encoded_path() {
    let data = b"\x1b]7;file://host/home/my%20folder\x1b\\";
    assert_eq!(detect_osc7_cwd(data), Some("/home/my folder".to_string()));
}

#[test]
fn test_detect_osc7_empty_hostname() {
    let data = b"\x1b]7;file:///home/user\x1b\\";
    assert_eq!(detect_osc7_cwd(data), Some("/home/user".to_string()));
}

// ---- URL Decode Tests ----

#[test]
fn test_url_decode_no_encoding() {
    assert_eq!(url_decode("/home/user"), "/home/user");
}

#[test]
fn test_url_decode_space() {
    assert_eq!(url_decode("/home/my%20folder"), "/home/my folder");
}

#[test]
fn test_url_decode_multiple() {
    assert_eq!(url_decode("/a%20b%2Fc"), "/a b/c");
}

#[test]
fn test_url_decode_multibyte_utf8() {
    assert_eq!(
        url_decode("/home/user/%E4%B8%AD%E6%96%87"),
        "/home/user/中文"
    );
}

// ── mux-snapshot-output-boundary task0002: reader-level reattach tests
//    (AC-2 "Oversize pane" reader case, AC-3 through AC-6) ────────────────
//
// These drive the PRODUCTION `collect_reattach_data` /
// `handle_request_pane_snapshot` against a pane registered in a real
// `SessionManager`, with the reader's handles cloned from that SAME pane —
// so the snapshot path observes exactly the state the reader writes (task
// plan Design section "Reader-level reattach harness").

use crate::mux::session::manager::SessionManager;
use tokio::sync::{Mutex, oneshot};

/// Register `pane` in a fresh `SessionManager` session/window and start its
/// reader thread against `chunks`, mirroring [`spawn_reader_with_chunks`]
/// but with the pane owned by a session so `collect_reattach_data` /
/// `handle_request_pane_snapshot` can be driven against it directly.
/// `pane`'s `output_target` is used exactly as given by the caller
/// (`Detached` for the visible-reattach scenarios below, `Connected` for
/// the on-demand ones) — this does not change `spawn_reader_with_chunks`'s
/// own behavior.
async fn spawn_reader_with_chunks_in_session(
    pane: MuxPane,
    chunks: Vec<Vec<u8>>,
) -> (Arc<Mutex<SessionManager>>, u32, std::thread::JoinHandle<()>) {
    let pane_id = pane.id;
    let output_target = pane.output_target.clone();
    let shadow_parser = pane.shadow_parser.clone();
    let cwd = pane.cwd.clone();
    let title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let dims = pane.dims.clone();
    let output_capture = pane.output_capture.clone();

    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        sid
    };

    let handle = std::thread::spawn(move || {
        pty_reader_loop(
            pane_id,
            Box::new(ScriptedReader::new(chunks)),
            output_target,
            shadow_parser,
            cwd,
            title,
            title_sender,
            notification_sender,
            agent_status_report_sender,
            raw_passthrough,
            passthrough_scanner,
            scrollback,
            dims,
            Arc::new(StdMutex::new(None)),
            output_capture,
        );
    });

    (mgr, session_id, handle)
}

/// AC-2 (FR3, FR5, NFR2, NFR3; TS-16), "Oversize pane": for a pane whose
/// assembled snapshot exceeds the single-frame limit, a visible reattach
/// through the production `collect_reattach_data` records NO boundary for
/// the new destination, and — with the reader paused at P2 during the
/// reattach — the paused chunk reaches the new destination's channel
/// normally after release (the swap to `Connected` still happens; nothing
/// suppresses the chunk because nothing was recorded for it).
#[tokio::test]
async fn collect_reattach_data_records_no_boundary_for_an_oversize_pane_and_the_reader_forwards_normally()
 {
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(20, 80, 24, output_target);
    // Oversize the pane's ring directly (mirrors the existing oversize
    // resume-path fixtures in `mux::session::pane::tests`): a
    // capacity-and-content-matched ring whose read alone exceeds
    // `MAX_SNAPSHOT_FRAME_PAYLOAD`.
    let oversize_capacity = mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1024 * 1024;
    *pane.scrollback.lock().unwrap() =
        crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(oversize_capacity);
    pane.scrollback
        .lock()
        .unwrap()
        .write(&vec![b'x'; oversize_capacity]);

    let output_capture = pane.output_capture.clone();
    let paused_chunk = b"plain output text after the oversize reattach".to_vec();
    let (arrived_rx, release_tx) = output_capture.p2.arm();

    let (mgr, session_id, handle) =
        spawn_reader_with_chunks_in_session(pane, vec![paused_chunk.clone()]).await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;
    assert_eq!(data.len(), 1);

    assert!(
        !output_capture.is_boundary_covered(&new_tx, 0),
        "an oversize pane's reattach must record no boundary for the new \
         destination"
    );

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert!(
        received.iter().any(|c| c.data == paused_chunk),
        "with no boundary recorded, the reader must forward the paused \
         chunk normally to the new destination after the swap to Connected"
    );
}

/// AC-3 (FR1, FR2, FR3; TS-1): with the reader paused at P2 on a chunk
/// containing an insert-line sequence (`CSI L`) on a Detached pane, a
/// visible reattach through the production `collect_reattach_data`
/// followed by release leaves the new destination's channel without that
/// chunk's raw bytes (the double-application fix). The client view
/// (returned snapshot, then every later channel chunk) matches the
/// raw-stream reference.
#[tokio::test]
async fn visible_reattach_through_collect_reattach_data_suppresses_the_paused_insert_line_chunk_and_matches_the_reference()
 {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(21, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    let baseline: &[u8] = b"line1\r\nline2\r\nline3\r\n";
    // Seed the baseline the SAME way the reader would (through the capture
    // step), so the paused chunk becomes capture step #2, matching a
    // realistic "some output already happened before this reattach"
    // scenario.
    output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(baseline);
        pane.scrollback.lock().unwrap().write(baseline);
    });

    let paused_chunk = b"\x1b[1;1H\x1b[L".to_vec(); // move to top-left, insert a line
    let continuation_chunk = b"more output after resume\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the paused chunk");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;
    assert_eq!(data.len(), 1);
    let (_pane_id, snapshot_payload, snapshot_segments) = data.into_iter().next().unwrap();

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert!(
        !received.iter().any(|c| c.data == paused_chunk),
        "the paused insert-line chunk's raw bytes must never reach the new \
         destination after the reattach snapshot (TS-1 double-application \
         fix)"
    );
    assert_eq!(
        received[0].data, continuation_chunk,
        "the next (unsuppressed) chunk must follow directly — the \
         suppressed chunk carried no query and no tail"
    );
    assert!(received[1].data.is_empty(), "EOF chunk must follow");
    assert_eq!(received.len(), 2, "nothing else must have been delivered");

    use term_core::terminal_core::TerminalCore;
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&snapshot_payload, &to_replay_segments(&snapshot_segments));
    client.process_pty_data_fully(&continuation_chunk);

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(baseline);
    reference.process_pty_data_fully(&paused_chunk);
    reference.process_pty_data_fully(&continuation_chunk);

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
    assert_eq!(client.get_cursor_row(), reference.get_cursor_row());
    assert_eq!(client.get_cursor_col(), reference.get_cursor_col());
}

/// AC-4 (FR9; TS-9), reader level, visible-reattach path (production
/// `collect_reattach_data`): "Device queries" + "String payloads" bullets.
/// A suppressed chunk containing a cursor-position query (`CSI 6n`) and a
/// primary device-attributes query (`CSI c`), plus a DCS string whose body
/// contains query-shaped bytes, delivers ONLY the two real queries, in
/// order, exactly once, after the reattach and before the next reader
/// chunk — the DCS payload's embedded query-shaped bytes are never
/// extracted (TM-1).
#[tokio::test]
async fn visible_reattach_redelivers_device_queries_once_in_order_and_never_from_inside_a_dcs_payload()
 {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(50, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    let mut paused_chunk = Vec::new();
    paused_chunk.extend_from_slice(b"before");
    paused_chunk.extend_from_slice(b"\x1b[6n");
    paused_chunk.extend_from_slice(b"middle");
    paused_chunk.extend_from_slice(b"\x1b[c");
    // DCS string whose payload contains query-shaped bytes — must never be
    // extracted as a query (TM-1).
    paused_chunk.extend_from_slice(b"\x1bPq\x1b[6n\x1b\\");
    paused_chunk.extend_from_slice(b"after\r\n");
    let continuation_chunk = b"final line\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let _data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].data, b"\x1b[6n\x1b[c\x1b[6n",
        "the two plain device queries plus the DCS-embedded one: the \
         embedded ESC aborts the DCS string (as-03), and the bytes that \
         follow it — \"[6n\" — are a genuine, freshly-dispatched CSI query, \
         not payload"
    );
    assert_eq!(received[1].data, continuation_chunk);
    assert!(received[2].data.is_empty());
    assert_eq!(received.len(), 3, "nothing else must have been delivered");
}

/// AC-4 (FR9; TS-9), reader level, visible-reattach path: "Alternate-screen
/// color query" bullet, changed on purpose by mux-suppressed-output-round2-fixes
/// FR2 (SPEC AC-8). A color query inside an alternate-screen span of a
/// suppressed chunk is re-delivered exactly once, and so is a color query in
/// the same kind of chunk that reached the ring (main-buffer): the snapshot
/// is replayed with its responses discarded, so a query that survived into
/// the ring is never answered through the snapshot.
#[tokio::test]
async fn visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once() {
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(51, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();

    // Chunk 1: enters the alternate screen and issues a color query there —
    // never written toward the ring (alt-screen has no scrollback).
    let mut alt_chunk = Vec::new();
    alt_chunk.extend_from_slice(b"\x1b[?1049h");
    alt_chunk.extend_from_slice(b"\x1b]11;?\x07");
    // Chunk 2: leaves the alternate screen, then issues the SAME color
    // query in the main buffer — this one IS written toward the ring.
    let mut main_chunk = Vec::new();
    main_chunk.extend_from_slice(b"\x1b[?1049l");
    main_chunk.extend_from_slice(b"\x1b]11;?\x07");

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) =
        spawn_reader_with_chunks_in_session(pane, vec![alt_chunk.clone(), main_chunk.clone()])
            .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the alt-screen chunk");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx1, _kick_rx1) = oneshot::channel::<()>();
    let _data1 = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx1, true, 10_000,
    )
    .await;
    release_tx.send(()).unwrap();

    // P2 disarms itself after one hit; re-arm for the main-buffer chunk and
    // reattach the SAME destination a second time (mirrors a second
    // window-switch back to the still-attached client).
    let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();
    arrived_rx_2
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the main-buffer chunk");
    let (kick_tx2, _kick_rx2) = oneshot::channel::<()>();
    let _data2 = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx2, true, 10_000,
    )
    .await;
    release_tx_2.send(()).unwrap();

    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].data, b"\x1b]11;?\x07",
        "the alt-screen color query must be re-delivered exactly once"
    );
    assert_eq!(
        received[1].data, b"\x1b]11;?\x07",
        "the main-buffer color query reached the ring, but the snapshot's \
         replay discards responses, so it must be re-delivered exactly once too"
    );
    assert!(received[2].data.is_empty(), "EOF chunk must follow");
    assert_eq!(received.len(), 3);
}

/// AC-4 (FR9; TS-9), reader level, on-demand path (production
/// `handle_request_pane_snapshot`): the on-demand counterpart of
/// `visible_reattach_redelivers_device_queries_once_in_order_and_never_from_inside_a_dcs_payload`.
#[tokio::test]
async fn on_demand_snapshot_redelivers_device_queries_once_in_order_and_never_from_inside_a_dcs_payload()
 {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let pane_id: PaneId = 52;
    let (requester_tx, mut requester_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        requester_tx.clone(),
    )));
    let pane = MuxPane::new_test(pane_id, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    let mut paused_chunk = Vec::new();
    paused_chunk.extend_from_slice(b"before");
    paused_chunk.extend_from_slice(b"\x1b[6n");
    paused_chunk.extend_from_slice(b"middle");
    paused_chunk.extend_from_slice(b"\x1b[c");
    paused_chunk.extend_from_slice(b"\x1bPq\x1b[6n\x1b\\");
    paused_chunk.extend_from_slice(b"after\r\n");
    let continuation_chunk = b"final line\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");

    let req = mux_ipc::protocol::MuxMessage {
        msg_type: mux_ipc::protocol::MessageType::RequestPaneSnapshot,
        pane_id,
        payload: Vec::new(),
    };
    let mut deferred = crate::mux::session::pane::DeferredOutputQueue::new();
    crate::mux::ipc::handlers::handle_request_pane_snapshot(
        &req,
        session_id,
        &mgr,
        &requester_tx,
        &mut deferred,
        10_000,
    )
    .await
    .expect("handle_request_pane_snapshot");

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = requester_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].kind,
        crate::mux::session::pane::ChunkKind::Snapshot,
        "the on-demand snapshot itself must arrive first"
    );
    assert_eq!(
        received[1].data, b"\x1b[6n\x1b[c\x1b[6n",
        "the two plain device queries plus the DCS-embedded one: the \
         embedded ESC aborts the DCS string (as-03), and the bytes that \
         follow it — \"[6n\" — are a genuine, freshly-dispatched CSI query, \
         not payload"
    );
    assert_eq!(received[2].data, continuation_chunk);
    assert!(received[3].data.is_empty());
    assert_eq!(received.len(), 4, "nothing else must have been delivered");
}

/// AC-6 (FR9, TM-3, NFR1; TS-11), reader level, on-demand path: a
/// suppressed chunk containing `A ESC[6n B ESC[c` delivers both queries
/// after the snapshot and before the next chunk, in order, once each, as a
/// single `PtyOutput`-kind chunk sent only to the covering snapshot's
/// destination. The client model produces exactly two responses.
#[cfg(feature = "gui")]
#[tokio::test]
async fn suppressed_queries_arrive_after_snapshot_in_order_once_each() {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let pane_id: PaneId = 54;
    let (requester_tx, mut requester_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        requester_tx.clone(),
    )));
    let pane = MuxPane::new_test(pane_id, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    let mut paused_chunk = Vec::new();
    paused_chunk.extend_from_slice(b"A");
    paused_chunk.extend_from_slice(b"\x1b[6n");
    paused_chunk.extend_from_slice(b"B");
    paused_chunk.extend_from_slice(b"\x1b[c");
    let continuation_chunk = b"final line\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");

    let req = mux_ipc::protocol::MuxMessage {
        msg_type: mux_ipc::protocol::MessageType::RequestPaneSnapshot,
        pane_id,
        payload: Vec::new(),
    };
    let mut deferred = crate::mux::session::pane::DeferredOutputQueue::new();
    crate::mux::ipc::handlers::handle_request_pane_snapshot(
        &req,
        session_id,
        &mgr,
        &requester_tx,
        &mut deferred,
        10_000,
    )
    .await
    .expect("handle_request_pane_snapshot");

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = requester_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].kind,
        crate::mux::session::pane::ChunkKind::Snapshot,
        "the snapshot itself must arrive first"
    );
    assert_eq!(
        received[1].kind,
        crate::mux::session::pane::ChunkKind::PtyOutput,
        "the replacement is a single PtyOutput-kind chunk"
    );
    assert_eq!(
        received[1].data, b"\x1b[6n\x1b[c",
        "both queries, in order, once each"
    );
    assert_eq!(received[2].data, continuation_chunk);
    assert!(received[3].data.is_empty());
    assert_eq!(
        received.len(),
        4,
        "nothing else must have been delivered — a \
        single PtyOutput-kind replacement chunk, sent only to the covering \
        snapshot's destination"
    );

    // The client model produces exactly two responses when fed the
    // replacement alone.
    use crate::callbacks::{NativeCallbackState, ThemeColorResponder};
    use crate::render::theme::Theme;
    use parking_lot::Mutex as PLMutex;
    use term_core::terminal_core::TerminalCore;
    let theme = Arc::new(PLMutex::new(Theme::default()));
    let state = Arc::new(PLMutex::new(NativeCallbackState::default()));
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.osc_responder = Some(Box::new(ThemeColorResponder::new(theme, state)));
    client.process_pty_data_fully(&received[1].data);
    let responses = client.take_response();
    // Two independent device-query responses (CPR + DA1) are each their
    // own complete CSI sequence terminated by a distinct final byte ('R'
    // and 'c' respectively) — count them by their final bytes rather than
    // assuming a fixed length.
    let response_count = responses
        .iter()
        .filter(|&&b| b == b'R' || b == b'c')
        .count();
    assert_eq!(
        response_count,
        2,
        "exactly two responses, one per query: {:?}",
        String::from_utf8_lossy(&responses)
    );
}

/// AC-4 (FR9; TS-9), reader level, on-demand path: the on-demand
/// counterpart of
/// `visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`
/// (changed on purpose by mux-suppressed-output-round2-fixes FR2, SPEC AC-8).
#[tokio::test]
async fn on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once() {
    let pane_id: PaneId = 53;
    let (requester_tx, mut requester_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        requester_tx.clone(),
    )));
    let pane = MuxPane::new_test(pane_id, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();

    let mut alt_chunk = Vec::new();
    alt_chunk.extend_from_slice(b"\x1b[?1049h");
    alt_chunk.extend_from_slice(b"\x1b]11;?\x07");
    let mut main_chunk = Vec::new();
    main_chunk.extend_from_slice(b"\x1b[?1049l");
    main_chunk.extend_from_slice(b"\x1b]11;?\x07");

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) =
        spawn_reader_with_chunks_in_session(pane, vec![alt_chunk.clone(), main_chunk.clone()])
            .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the alt-screen chunk");
    let mut deferred = crate::mux::session::pane::DeferredOutputQueue::new();
    let req = mux_ipc::protocol::MuxMessage {
        msg_type: mux_ipc::protocol::MessageType::RequestPaneSnapshot,
        pane_id,
        payload: Vec::new(),
    };
    crate::mux::ipc::handlers::handle_request_pane_snapshot(
        &req,
        session_id,
        &mgr,
        &requester_tx,
        &mut deferred,
        10_000,
    )
    .await
    .expect("handle_request_pane_snapshot");
    release_tx.send(()).unwrap();

    let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();
    arrived_rx_2
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the main-buffer chunk");
    crate::mux::ipc::handlers::handle_request_pane_snapshot(
        &req,
        session_id,
        &mgr,
        &requester_tx,
        &mut deferred,
        10_000,
    )
    .await
    .expect("handle_request_pane_snapshot");
    release_tx_2.send(()).unwrap();

    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = requester_rx.try_recv() {
        received.push(c);
    }
    let snapshots: Vec<&PtyOutputChunk> = received
        .iter()
        .filter(|c| c.kind == crate::mux::session::pane::ChunkKind::Snapshot)
        .collect();
    assert_eq!(snapshots.len(), 2, "one snapshot per on-demand request");

    let non_snapshot: Vec<&PtyOutputChunk> = received
        .iter()
        .filter(|c| c.kind != crate::mux::session::pane::ChunkKind::Snapshot)
        .collect();
    assert_eq!(
        non_snapshot[0].data, b"\x1b]11;?\x07",
        "the alt-screen color query must be re-delivered exactly once"
    );
    assert_eq!(
        non_snapshot[1].data, b"\x1b]11;?\x07",
        "the main-buffer color query reached the ring, but the snapshot's \
         replay discards responses, so it must be re-delivered exactly once too"
    );
    assert!(non_snapshot[2].data.is_empty(), "EOF chunk must follow");
    assert_eq!(non_snapshot.len(), 3);
}

// ── mux-suppressed-output-round2-fixes task0002: main-screen color queries
//    (FR2, review finding `dd56f3984c74cde1`) ─────────────────────────────

/// A term_core client wired with the real theme's OSC responder — the
/// model of what the GUI does with a color query.
#[cfg(feature = "gui")]
fn themed_client(cols: u16, rows: u16) -> term_core::terminal_core::TerminalCore {
    use crate::callbacks::{NativeCallbackState, ThemeColorResponder};
    use crate::render::theme::Theme;
    use parking_lot::Mutex as PLMutex;

    let theme = Arc::new(PLMutex::new(Theme::default()));
    let state = Arc::new(PLMutex::new(NativeCallbackState::default()));
    let mut client = term_core::terminal_core::TerminalCore::new(cols, rows, 10_000);
    client.osc_responder = Some(Box::new(ThemeColorResponder::new(theme, state)));
    client
}

/// Decode a `Snapshot`-kind chunk's wire-encoded bytes into the payload and
/// the replay segments `reset_and_replay_segments` takes.
#[cfg(feature = "gui")]
fn decode_snapshot_for_replay(data: &[u8]) -> (Vec<u8>, Vec<ReplaySegment>) {
    let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(data);
    let replay = segments
        .iter()
        .map(|s| ReplaySegment {
            offset: s.offset,
            cols: s.cols,
            rows: s.rows,
        })
        .collect();
    (content.to_vec(), replay)
}

/// What one snapshot path delivered to the destination around a suppressed
/// main-screen chunk.
#[cfg(feature = "gui")]
struct SnapshotFlow {
    /// The snapshot's replayable bytes.
    payload: Vec<u8>,
    /// The snapshot's replay segments.
    segments: Vec<ReplaySegment>,
    /// The data of every chunk the destination received after the snapshot,
    /// in order, EOF excluded.
    after: Vec<Vec<u8>>,
}

/// The client model of a snapshot path: the snapshot is applied with
/// `reset_and_replay_segments` and every response it produced is discarded
/// (as the GUI does), then the delivered chunks are fed in order. Returns
/// the responses queued after the snapshot.
#[cfg(feature = "gui")]
fn responses_after_snapshot(flow: &SnapshotFlow) -> Vec<u8> {
    let mut client = themed_client(80, 24);
    client.reset_and_replay_segments(&flow.payload, &flow.segments);
    let _discarded_replay_responses = client.take_response();
    for chunk in &flow.after {
        client.process_pty_data_fully(chunk);
    }
    client.take_response()
}

/// The number of OSC 11 responses in `responses`.
#[cfg(feature = "gui")]
fn osc11_response_count(responses: &[u8]) -> usize {
    let prefix = b"\x1b]11;";
    responses
        .windows(prefix.len())
        .filter(|w| *w == prefix)
        .count()
}

/// Spawn `pty_reader_loop` against `pane` exactly as it is — unlike
/// [`spawn_reader_with_chunks`] this leaves the pane's `output_target`
/// (`Detached` for the visibility-restore scenario) untouched.
#[cfg(feature = "gui")]
fn spawn_reader_leaving_target_untouched(
    pane: &MuxPane,
    chunks: Vec<Vec<u8>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn({
        let output_target = pane.output_target.clone();
        let shadow_parser = pane.shadow_parser.clone();
        let cwd = pane.cwd.clone();
        let title = pane.title.clone();
        let title_sender = pane.title_sender.clone();
        let notification_sender = pane.notification_sender.clone();
        let agent_status_report_sender = pane.agent_status_report_sender.clone();
        let raw_passthrough = pane.raw_passthrough.clone();
        let passthrough_scanner = pane.passthrough_scanner.clone();
        let scrollback = pane.scrollback.clone();
        let dims = pane.dims.clone();
        let output_capture = pane.output_capture.clone();
        let pane_id = pane.id;
        move || {
            pty_reader_loop(
                pane_id,
                Box::new(ScriptedReader::new(chunks)),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                output_capture,
            );
        }
    })
}

/// Visible-reattach path (production `collect_reattach_data`): the reader is
/// paused at P2 on `chunk` (a main-screen chunk), the reattach snapshot
/// covers it, and the reader's suppression decision delivers whatever the
/// replacement carries.
#[cfg(feature = "gui")]
async fn main_screen_flow_via_visible_reattach(pane_id: PaneId, chunk: &[u8]) -> SnapshotFlow {
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(pane_id, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();
    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) =
        spawn_reader_with_chunks_in_session(pane, vec![chunk.to_vec()]).await;
    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the main-screen chunk");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;
    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let (_pane_id, payload, segments) = data.into_iter().next().expect("one pane's snapshot");
    let mut after = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        if !c.data.is_empty() {
            after.push(c.data);
        }
    }
    SnapshotFlow {
        payload,
        segments: to_replay_segments(&segments),
        after,
    }
}

/// On-demand path (production `handle_request_pane_snapshot`).
#[cfg(feature = "gui")]
async fn main_screen_flow_via_on_demand_snapshot(pane_id: PaneId, chunk: &[u8]) -> SnapshotFlow {
    let (requester_tx, mut requester_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
        requester_tx.clone(),
    )));
    let pane = MuxPane::new_test(pane_id, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();
    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) =
        spawn_reader_with_chunks_in_session(pane, vec![chunk.to_vec()]).await;
    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the main-screen chunk");

    let req = mux_ipc::protocol::MuxMessage {
        msg_type: mux_ipc::protocol::MessageType::RequestPaneSnapshot,
        pane_id,
        payload: Vec::new(),
    };
    let mut deferred = crate::mux::session::pane::DeferredOutputQueue::new();
    crate::mux::ipc::handlers::handle_request_pane_snapshot(
        &req,
        session_id,
        &mgr,
        &requester_tx,
        &mut deferred,
        10_000,
    )
    .await
    .expect("handle_request_pane_snapshot");
    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = requester_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].kind,
        crate::mux::session::pane::ChunkKind::Snapshot,
        "the on-demand snapshot itself must arrive first"
    );
    let (payload, segments) = decode_snapshot_for_replay(&received[0].data);
    let after = received[1..]
        .iter()
        .filter(|c| !c.data.is_empty())
        .map(|c| c.data.clone())
        .collect();
    SnapshotFlow {
        payload,
        segments,
        after,
    }
}

/// Visibility-restore path (production `resume_pane_with_permit`): a hidden
/// (`Detached { HiddenByVisibility }`) pane whose reader is paused at P2 on
/// `chunk` is made visible again.
#[cfg(feature = "gui")]
fn main_screen_flow_via_visibility_restore(pane_id: PaneId, chunk: &[u8]) -> SnapshotFlow {
    use crate::mux::session::pane::{AnyPermit, ResumeOutcome, resume_pane_with_permit};

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(new_tx.clone()),
    }));
    let pane = MuxPane::new_test(pane_id, 80, 24, output_target);
    let output_capture = pane.output_capture.clone();
    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let handle = spawn_reader_leaving_target_untouched(&pane, vec![chunk.to_vec()]);
    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the main-screen chunk");

    let permit = new_tx.try_reserve().expect("a slot in the fresh channel");
    let outcome = resume_pane_with_permit(&pane, &new_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(
        matches!(outcome, ResumeOutcome::Resumed),
        "the hidden pane must resume"
    );
    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].kind,
        crate::mux::session::pane::ChunkKind::Snapshot,
        "the visibility-restore snapshot itself must arrive first"
    );
    let (payload, segments) = decode_snapshot_for_replay(&received[0].data);
    let after = received[1..]
        .iter()
        .filter(|c| !c.data.is_empty())
        .map(|c| c.data.clone())
        .collect();
    SnapshotFlow {
        payload,
        segments,
        after,
    }
}

/// AC-5 (FR2, TS-2; regression test for review round 2 finding
/// `dd56f3984c74cde1`): on the main screen, a suppressed chunk holding
/// `ESC ]11;?` BEL is followed by the snapshot (the real assembly output,
/// applied with `reset_and_replay_segments`, responses discarded) and then
/// the replacement output, and the client answers exactly once. Checked
/// without the reader (real snapshot assembly + the replacement builder),
/// then through the reader for visible reattach, on-demand snapshot and
/// visibility restore.
#[cfg(feature = "gui")]
#[tokio::test]
async fn round2_dd56f398_main_screen_color_query_is_answered_once_after_the_snapshot() {
    let query: &[u8] = b"\x1b]11;?\x07";

    // Reference: the raw stream fed once is answered once.
    let mut reference = themed_client(80, 24);
    reference.process_pty_data_fully(query);
    assert_eq!(
        osc11_response_count(&reference.take_response()),
        1,
        "premise: the client answers the raw color query once"
    );

    // Without the reader: the ring and the shadow parser hold the chunk (a
    // main-screen chunk is written toward the ring), the snapshot is the
    // real assembly output, and the replacement is the real builder's.
    let model_flow = {
        let output_target: SharedOutputTarget = Arc::new(StdMutex::new(
            PaneOutputTarget::Connected(mpsc::channel(1).0),
        ));
        let pane = MuxPane::new_test(70, 80, 24, output_target);
        pane.scrollback.lock().unwrap().write(query);
        pane.shadow_parser.lock().unwrap().process(query);
        let (scrollback_data, scrollback_segments, ring_wrapped) = pane
            .scrollback
            .lock()
            .unwrap()
            .read_segments_with_wrap_state();
        let (screen_data, alt_screen, current_dims) = {
            let parser = pane.shadow_parser.lock().unwrap();
            let screen = parser.screen();
            let (rows, cols) = screen.size();
            (
                screen.contents_formatted(),
                screen.alternate_screen(),
                (cols, rows),
            )
        };
        let (payload, segments) = crate::mux::snapshot_bytes::build_snapshot_bytes_for_ring(
            &scrollback_data,
            &scrollback_segments,
            &screen_data,
            alt_screen,
            ring_wrapped,
            current_dims,
            10_000,
        );
        let replacement =
            suppressed_output::build_suppressed_replacement(query, &[0..query.len()], &[], &[]);
        SnapshotFlow {
            payload,
            segments: to_replay_segments(&segments),
            after: if replacement.is_empty() {
                Vec::new()
            } else {
                vec![replacement]
            },
        }
    };

    let flows = [
        ("real assembly + builder", model_flow),
        (
            "visible reattach",
            main_screen_flow_via_visible_reattach(71, query).await,
        ),
        (
            "on-demand snapshot",
            main_screen_flow_via_on_demand_snapshot(72, query).await,
        ),
        (
            "visibility restore",
            main_screen_flow_via_visibility_restore(73, query),
        ),
    ];
    // Every flow is judged (no early abort) so a failure names all of them.
    let mut failures = Vec::new();
    for (label, flow) in &flows {
        let responses = responses_after_snapshot(flow);
        let answered = osc11_response_count(&responses);
        if answered != 1 {
            failures.push(format!(
                "{label}: the main-screen color query was answered {answered} times \
                 after the snapshot, expected exactly once"
            ));
        }
        if flow.after != vec![query.to_vec()] {
            failures.push(format!(
                "{label}: expected only the replacement's copy of the query after \
                 the snapshot, got {:?}",
                flow.after
                    .iter()
                    .map(|c| String::from_utf8_lossy(c).into_owned())
                    .collect::<Vec<_>>()
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ── mux-suppressed-output-round2-fixes task0002: DECISIONS.md contract
//    (FR9, AC-7) ─────────────────────────────────────────────────────────

/// The round-2 decision table, read at run time from the repository's
/// `feature-docs/mux-suppressed-output-round2-fixes/DECISIONS.md` (found
/// from `CARGO_MANIFEST_DIR`).
fn round2_decisions_md() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("feature-docs")
        .join("mux-suppressed-output-round2-fixes")
        .join("DECISIONS.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The lines of `doc` under the `## <heading>` heading, up to the next
/// `## ` heading.
fn round2_decisions_section<'a>(doc: &'a str, heading: &str) -> Vec<&'a str> {
    let marker = format!("## {heading}");
    doc.lines()
        .skip_while(|l| l.trim_end() != marker)
        .skip(1)
        .take_while(|l| !l.starts_with("## "))
        .collect()
}

/// The data rows of the table in `lines`: each row's trimmed cells, the
/// header row and the `|---|` separator excluded.
fn round2_decisions_table_rows(lines: &[&str]) -> Vec<Vec<String>> {
    lines
        .iter()
        .filter(|l| l.starts_with('|') && !l.contains("---"))
        .map(|l| {
            l.trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect::<Vec<String>>()
        })
        .skip(1)
        .collect()
}

/// IMPLEMENTATION.md "Regression test registry": `(stable_id, requirement,
/// file, test name)` in the order FR1 through FR8.
const ROUND2_REGISTRY: [(&str, &str, &str, &str); 8] = [
    (
        "ecc48041b65a5380",
        "FR1",
        "src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs",
        "round2_ecc48041_designator_slot_esc_is_never_a_window_restart_position",
    ),
    (
        "dd56f3984c74cde1",
        "FR2",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
        "round2_dd56f398_main_screen_color_query_is_answered_once_after_the_snapshot",
    ),
    (
        "ae48e7cd98084c19",
        "FR3",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
        "round2_ae48e7cd_awaiting_designator_is_carried_across_feeds",
    ),
    (
        "b600645f1fa94686",
        "FR4",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
        "round2_b600645f_removed_screen_switch_closes_the_pending_string",
    ),
    (
        "f8b600bcc0ed55da",
        "FR5",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
        "round2_f8b600bc_pending_exclusion_keeps_alt_screen_queries",
    ),
    (
        "03ccd5c7702db8db",
        "FR6",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
        "round2_03ccd5c7_carried_over_viewer_launch_is_delivered_once",
    ),
    (
        "a93dffe30438a693",
        "FR7",
        "src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs",
        "round2_a93dffe3_leading_zero_viewer_launch_reaches_the_client_once",
    ),
    (
        "3eccc254dd278b33",
        "FR8",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
        "round2_3eccc254_visibility_restore_does_not_resend_a_tail_the_snapshot_carried",
    ),
];

/// The tests whose expectation changed on purpose (SPEC AC-8): `(old name,
/// new name, file)`.
const ROUND2_BEHAVIOR_CHANGING_TESTS: [(&str, &str, &str); 3] = [
    (
        "osc_color_query_inside_ring_written_ranges_is_not_redelivered",
        "osc_color_query_inside_ring_written_ranges_is_redelivered_once",
        "src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs",
    ),
    (
        "visible_reattach_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring",
        "visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
    ),
    (
        "on_demand_snapshot_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring",
        "on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once",
        "src-tauri/src/mux/ipc/pty_spawn/tests.rs",
    ),
];

/// AC-7 (FR9): the decision table has exactly eight rows, one per
/// stable_id in the order FR1 through FR8, each with the requirement, the
/// verdict "resolved", a rationale, and the registry test's file and name.
#[test]
fn round2_decisions_md_has_one_resolved_row_per_finding_with_its_registry_test() {
    let doc = round2_decisions_md();
    let section = round2_decisions_section(&doc, "Decision table");
    let rows = round2_decisions_table_rows(&section);
    assert_eq!(rows.len(), 8, "expected eight rows: {rows:?}");
    for (row, (stable_id, requirement, file, test)) in rows.iter().zip(ROUND2_REGISTRY) {
        assert_eq!(
            row.len(),
            5,
            "row for {stable_id} must have 5 columns (stable_id, requirement, \
             verdict, rationale, regression test): {row:?}"
        );
        assert_eq!(row[0], stable_id, "rows must follow the order FR1..FR8");
        assert_eq!(row[1], requirement, "requirement of {stable_id}");
        assert_eq!(row[2], "resolved", "verdict of {stable_id}");
        assert!(
            !row[3].is_empty(),
            "row for {stable_id} must carry a rationale"
        );
        assert!(
            row[4].contains(file) && row[4].contains(test),
            "row for {stable_id} must cite the registry file {file:?} and test \
             {test:?}: {}",
            row[4]
        );
    }
}

/// AC-7 (FR9, SPEC AC-8): the tests whose expectation changed on purpose
/// are listed with old name, new name and file, and the new names are
/// defined in the source while the old ones are gone.
#[test]
fn round2_decisions_md_lists_the_behavior_changing_tests_and_they_exist_under_their_new_names() {
    let doc = round2_decisions_md();
    let section = round2_decisions_section(&doc, "Behavior-changing tests");
    let rows = round2_decisions_table_rows(&section);
    assert_eq!(rows.len(), 3, "expected three rows: {rows:?}");
    for (row, (old, new, file)) in rows.iter().zip(ROUND2_BEHAVIOR_CHANGING_TESTS) {
        assert!(
            row.len() >= 4,
            "row must have old name, new name, file and reason: {row:?}"
        );
        assert!(row[0].contains(old), "old name {old:?} in {row:?}");
        assert!(row[1].contains(new), "new name {new:?} in {row:?}");
        assert!(row[2].contains(file), "file {file:?} in {row:?}");
        assert!(!row[3].is_empty(), "reason in {row:?}");
        assert!(
            registry_test_name_is_defined_in_crate_source(new),
            "the renamed test {new:?} must be defined in the source"
        );
        assert!(
            !registry_test_name_is_defined_in_crate_source(old),
            "the old test name {old:?} must no longer be defined"
        );
    }
}

/// AC-7 (FR9): the document states that round2.yaml was not modified.
#[test]
fn round2_decisions_md_states_that_round2_yaml_was_not_modified() {
    let doc = round2_decisions_md();
    assert!(
        doc.lines().any(|l| l
            .contains("feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml")
            && l.contains("was not modified")),
        "DECISIONS.md must state that round2.yaml was not modified"
    );
}

/// AC-2, AC-5 (registry): the two registry tests this task owns are
/// defined under exactly the pinned names.
#[test]
fn round2_task0002_registry_tests_are_defined_under_their_pinned_names() {
    for (_, requirement, _, test) in ROUND2_REGISTRY {
        if requirement == "FR1" || requirement == "FR2" {
            assert!(
                registry_test_name_is_defined_in_crate_source(test),
                "{requirement}'s registry test {test:?} must be defined in the source"
            );
        }
    }
}

/// AC-5 (FR10; TS-10), "Cut CSI": under the AC-3 conditions (Detached pane,
/// visible reattach through the production `collect_reattach_data` with
/// the reader paused at P2), a suppressed chunk ending in a cut CSI has its
/// continuation in the next (unsuppressed) chunk. The client view matches
/// the raw-stream reference.
#[tokio::test]
async fn visible_reattach_redelivers_a_cut_csi_tail_and_the_client_view_matches_the_reference() {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(60, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    let paused_chunk = b"plain text\x1b[3".to_vec();
    let continuation_chunk = b"2m colored text\x1b[0m\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;
    let (_pane_id, snapshot_payload, snapshot_segments) = data.into_iter().next().unwrap();

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].data, b"\x1b[3",
        "the cut CSI tail must be re-delivered verbatim"
    );
    assert_eq!(received[1].data, continuation_chunk);
    assert!(received[2].data.is_empty());
    assert_eq!(received.len(), 3);

    use term_core::terminal_core::TerminalCore;
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&snapshot_payload, &to_replay_segments(&snapshot_segments));
    client.process_pty_data_fully(&received[0].data);
    client.process_pty_data_fully(&continuation_chunk);

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&paused_chunk);
    reference.process_pty_data_fully(&continuation_chunk);

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
    assert_eq!(client.get_cursor_row(), reference.get_cursor_row());
    assert_eq!(client.get_cursor_col(), reference.get_cursor_col());
}

/// AC-5 (FR10; TS-10), "Cut UTF-8 character": same shape as the cut-CSI
/// test above, for a multi-byte UTF-8 character split across the
/// suppressed chunk boundary. No U+FFFD replacement character may appear
/// in any visible row.
#[tokio::test]
async fn visible_reattach_redelivers_a_cut_utf8_tail_with_no_replacement_character_in_any_row() {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(61, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    let mut paused_chunk = b"plain".to_vec();
    paused_chunk.extend_from_slice(&[0xe4, 0xb8]); // first two bytes of "中" (E4 B8 AD)
    let mut continuation_chunk = vec![0xad];
    continuation_chunk.extend_from_slice(b" more\r\n");

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;
    let (_pane_id, snapshot_payload, snapshot_segments) = data.into_iter().next().unwrap();

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(received[0].data, vec![0xe4, 0xb8]);
    assert_eq!(received[1].data, continuation_chunk);
    assert!(received[2].data.is_empty());
    assert_eq!(received.len(), 3);

    use term_core::terminal_core::TerminalCore;
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&snapshot_payload, &to_replay_segments(&snapshot_segments));
    client.process_pty_data_fully(&received[0].data);
    client.process_pty_data_fully(&continuation_chunk);

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&paused_chunk);
    reference.process_pty_data_fully(&continuation_chunk);

    for r in 0..rows {
        let client_line = client.get_line_text(r);
        assert!(
            !client_line.contains('\u{FFFD}'),
            "row {r} contains a stray replacement character: {client_line:?}"
        );
        assert_eq!(
            client_line.trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
}

/// AC-5 (FR10; TS-10), "Pending rich-content candidate": the scrollback
/// write filter holds an unterminated OSC 9999 `emterm-md` introducer as a
/// pending run (D8); its continuation arrives in the next (unsuppressed)
/// chunk. No continuation byte or U+FFFD character may appear in any
/// visible row.
#[tokio::test]
async fn visible_reattach_redelivers_a_pending_rich_content_candidate_and_the_client_view_matches_the_reference()
 {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let output_target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }));
    let pane = MuxPane::new_test(62, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    // A plain-text prefix that DOES reach the ring/snapshot, followed by
    // the unterminated candidate the write filter holds back as `pending`
    // (D8) — the prefix is what makes this test SENSITIVE to suppression
    // being disabled: an unsuppressed forward would deliver the whole raw
    // chunk (prefix included), whereas the FR9/FR10 replacement for a
    // suppressed chunk re-delivers ONLY the pending run, since the prefix
    // already reached the client via the snapshot.
    let prefix = b"already-visible-text".to_vec();
    let pending_introducer = b"\x1b]9999;emterm-md;partial-payload".to_vec();
    let mut paused_chunk = prefix.clone();
    paused_chunk.extend_from_slice(&pending_introducer);
    let continuation_chunk = b";more-payload\x07after text\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2");

    let (new_tx, mut new_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &new_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;
    let (_pane_id, snapshot_payload, snapshot_segments) = data.into_iter().next().unwrap();

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = new_rx.try_recv() {
        received.push(c);
    }
    assert_eq!(
        received[0].data, pending_introducer,
        "the tail must re-deliver ONLY the pending run (the unterminated \
         introducer) — the already-ring-written prefix must not be resent"
    );
    assert_eq!(received[1].data, continuation_chunk);
    assert!(received[2].data.is_empty());
    assert_eq!(received.len(), 3);

    use term_core::terminal_core::TerminalCore;
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&snapshot_payload, &to_replay_segments(&snapshot_segments));
    client.process_pty_data_fully(&received[0].data);
    client.process_pty_data_fully(&continuation_chunk);

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&paused_chunk);
    reference.process_pty_data_fully(&continuation_chunk);

    for r in 0..rows {
        let client_line = client.get_line_text(r);
        assert!(
            !client_line.contains('\u{FFFD}'),
            "row {r} contains a stray replacement character: {client_line:?}"
        );
        assert_eq!(
            client_line.trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
}

/// mux-suppressed-output-fixes task0002 AC-4 (FR5; TS-5): a FORWARDED
/// (unsuppressed) read containing an ESC-aborted OSC and an ESC-aborted DCS,
/// followed by a complete CSI device query (`ESC[6n`) and plain text, must
/// leave the write filter's `pending` EMPTY once it is fully processed — the
/// aborted strings are closed, not held, so nothing after them is trapped.
/// When the NEXT read is suppressed by a production on-demand snapshot
/// (`handle_request_pane_snapshot`), its FR9/FR10 replacement must not
/// resend that already-delivered query or text: with a buggy filter that
/// still treated the aborted OSC as unterminated, `pending` would still hold
/// the whole first chunk (query and text included) when the second chunk's
/// replacement is built, duplicating what the client already received raw.
/// The daemon's scrollback ring, built from two SEPARATE `feed` calls (one
/// per chunk), must equal a single strip of the whole concatenated
/// main-buffer stream — splitting across reads must not change the result.
#[tokio::test]
async fn suppressed_chunk_after_aborted_strings_does_not_resend_delivered_query_or_text() {
    let pane_id: PaneId = 70;
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));

    let pane = MuxPane::new_test(pane_id, 80, 24, output_target.clone());
    let shadow_parser = pane.shadow_parser.clone();
    let cwd = pane.cwd.clone();
    let title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let dims = pane.dims.clone();
    let output_capture = pane.output_capture.clone();

    let mgr = Arc::new(tokio::sync::Mutex::new(SessionManager::new()));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        sid
    };

    // chunk0: ESC-aborted OSC, ESC-aborted DCS, then a complete CSI device
    // query and plain text — meant to be delivered normally.
    let chunk0 = b"\x1b]0;osc\x1bPdcs\x1b[6ntext".to_vec();
    // chunk1: unrelated plain content — meant to be suppressed.
    let chunk1 = b"more-output".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let handle = std::thread::spawn({
        let output_target = output_target.clone();
        let shadow_parser = shadow_parser.clone();
        let cwd = cwd.clone();
        let title = title.clone();
        let title_sender = title_sender.clone();
        let notification_sender = notification_sender.clone();
        let agent_status_report_sender = agent_status_report_sender.clone();
        let raw_passthrough = raw_passthrough.clone();
        let passthrough_scanner = passthrough_scanner.clone();
        let scrollback = scrollback.clone();
        let dims = dims.clone();
        let output_capture = output_capture.clone();
        let chunks = vec![chunk0.clone(), chunk1.clone()];
        move || {
            pty_reader_loop(
                pane_id,
                Box::new(ScriptedReader::new(chunks)),
                output_target,
                shadow_parser,
                cwd,
                title,
                title_sender,
                notification_sender,
                agent_status_report_sender,
                raw_passthrough,
                passthrough_scanner,
                scrollback,
                dims,
                Arc::new(StdMutex::new(None)),
                output_capture,
            );
        }
    });

    // Pause on chunk0's own P2 (right after its capture — the write filter
    // has already been fed chunk0 by this point). Re-arm P2 for chunk1
    // BEFORE releasing chunk0, so the re-arm happens while the reader thread
    // is still blocked inside the OLD `hit()` call — no race with the
    // reader racing ahead to chunk1's own P2 first.
    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on chunk0");
    let (arrived_rx_2, release_tx_2) = output_capture.p2.arm();
    release_tx.send(()).unwrap();

    // chunk0 proceeds past P2 with no boundary recorded yet — forwarded
    // normally to the connected destination.

    arrived_rx_2
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on chunk1");

    // Drive the PRODUCTION on-demand snapshot path while paused on chunk1 —
    // its boundary now covers BOTH chunk0 and chunk1 (both already
    // captured), so chunk1 (the "next" read) will be suppressed once
    // released.
    let req = mux_ipc::protocol::MuxMessage {
        msg_type: mux_ipc::protocol::MessageType::RequestPaneSnapshot,
        pane_id,
        payload: Vec::new(),
    };
    let mut deferred = crate::mux::session::pane::DeferredOutputQueue::new();
    crate::mux::ipc::handlers::handle_request_pane_snapshot(
        &req,
        session_id,
        &mgr,
        &owned_tx,
        &mut deferred,
        10_000,
    )
    .await
    .expect("handle_request_pane_snapshot");
    assert!(deferred.is_empty(), "the channel has room; must not defer");

    release_tx_2.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }
    let received_data: Vec<&[u8]> = received.iter().map(|c| c.data.as_slice()).collect();
    assert_eq!(
        received.len(),
        3,
        "chunk0 (forwarded raw), the on-demand snapshot, then EOF — chunk1's \
         suppressed-chunk replacement must be EMPTY (no query, no tail) and \
         send nothing: {received_data:?}"
    );
    assert_eq!(
        received[0].kind,
        crate::mux::session::pane::ChunkKind::PtyOutput
    );
    assert_eq!(
        received[0].data, chunk0,
        "chunk0 must be forwarded verbatim, unsuppressed"
    );
    assert_eq!(
        received[1].kind,
        crate::mux::session::pane::ChunkKind::Snapshot
    );
    assert!(received[2].data.is_empty(), "EOF chunk must follow");

    // Restricted to PtyOutput-kind chunks: the Snapshot chunk's payload is
    // the RENDERED grid (built from the shadow parser's screen contents), so
    // it legitimately contains the literal text "text" already drawn by
    // chunk0 — that is not a "resend" via the suppressed-chunk pipeline this
    // assertion is about.
    let all_pty_output: Vec<u8> = received
        .iter()
        .filter(|c| c.kind == crate::mux::session::pane::ChunkKind::PtyOutput)
        .flat_map(|c| c.data.clone())
        .collect();
    let query_occurrences = all_pty_output
        .windows(4)
        .filter(|w| *w == b"\x1b[6n")
        .count();
    assert_eq!(
        query_occurrences, 1,
        "the CSI device query must reach the client exactly once (via \
         chunk0's normal forward), never resent by chunk1's replacement"
    );
    let text_occurrences = all_pty_output.windows(4).filter(|w| *w == b"text").count();
    assert_eq!(
        text_occurrences, 1,
        "the plain text after the query must reach the client exactly once, \
         never resent by chunk1's replacement"
    );

    let mut concatenated = chunk0.clone();
    concatenated.extend_from_slice(&chunk1);
    let expected_ring = strip_pty_output_for_scrollback_write(&concatenated);
    assert_eq!(
        scrollback.lock().unwrap().read_all(),
        expected_ring,
        "the ring, built from two separate feed() calls, must equal a \
         single strip of the whole concatenated main-buffer stream"
    );
}

/// AC-6 (FR11; TS-11): destination A holds a boundary for the pane ABOVE
/// the numbers of the chunks that follow (a stand-in for A having already
/// received a snapshot). Destination B then takes the pane over through
/// the production `collect_reattach_data(visible = true)` while the reader
/// is paused at P2. After release: every chunk numbered above B's own
/// captured boundary reaches B's channel — none suppressed by A's
/// (unrelated, larger) entry — B's client view matches the raw-stream
/// reference, and A's channel receives no chunk produced after the
/// takeover.
#[tokio::test]
async fn destination_takeover_through_collect_reattach_data_binds_suppression_to_the_new_sender_not_the_old()
 {
    let cols: u16 = 80;
    let rows: u16 = 24;
    let (a_tx, mut a_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let output_target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(a_tx.clone())));
    let pane = MuxPane::new_test(70, cols, rows, output_target);
    let output_capture = pane.output_capture.clone();

    // AC-6 Test Notes: A's boundary can be put in place with the existing
    // boundary-record operation directly, standing in for A having
    // received an earlier snapshot — deliberately far ABOVE the numbers
    // the two chunks below will get, so a broken (non-per-sender) lookup
    // would wrongly suppress everything for B too.
    output_capture.record_boundary(&a_tx, 1_000);

    let takeover_chunk = b"line during takeover\r\n".to_vec();
    let after_takeover_chunk = b"line after takeover, must reach B\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let (mgr, session_id, handle) = spawn_reader_with_chunks_in_session(
        pane,
        vec![takeover_chunk.clone(), after_takeover_chunk.clone()],
    )
    .await;

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the takeover chunk");

    let (b_tx, mut b_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
    let (kick_tx, _kick_rx) = oneshot::channel::<()>();
    let data = crate::mux::ipc::reattach::collect_reattach_data(
        &mgr, session_id, &b_tx, &title_tx, kick_tx, true, 10_000,
    )
    .await;
    assert_eq!(data.len(), 1);
    let (_pane_id, snapshot_payload, snapshot_segments) = data.into_iter().next().unwrap();

    assert!(
        output_capture.is_boundary_covered(&b_tx, 0),
        "B's takeover must record its own boundary entry"
    );

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received_b = Vec::new();
    while let Ok(c) = b_rx.try_recv() {
        received_b.push(c);
    }
    assert!(
        !received_b.iter().any(|c| c.data == takeover_chunk),
        "the takeover chunk itself must be suppressed for B (covered by \
         B's own just-recorded boundary)"
    );
    assert!(
        received_b.iter().any(|c| c.data == after_takeover_chunk),
        "a chunk numbered ABOVE B's boundary must reach B normally — never \
         suppressed by A's unrelated (larger, but different-sender) entry"
    );

    let mut received_a = Vec::new();
    while let Ok(c) = a_rx.try_recv() {
        received_a.push(c);
    }
    assert!(
        received_a.is_empty(),
        "A's channel must receive nothing produced after the takeover"
    );

    use term_core::terminal_core::TerminalCore;
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&snapshot_payload, &to_replay_segments(&snapshot_segments));
    client.process_pty_data_fully(&after_takeover_chunk);

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&takeover_chunk);
    reference.process_pty_data_fully(&after_takeover_chunk);

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
}

// ── End-to-end child reap (task0001 AC-1/AC-7; TS-7, TS-8, TS-9) ──────
//
// Uses the real spawn path (`spawn_pty`), builds a `MuxPane` directly
// from the resulting `SpawnedPty` (mirroring exactly what
// `register_pane_and_start_reader` does with `spawned.writer` /
// `spawned.master` / `spawned.child`), tears down via `mark_exited`,
// then polls the process table until the spawned PID is gone. Unix-only
// (`/proc` observation, SPEC A4); skips cleanly when the environment
// cannot open a PTY.
#[cfg(all(test, unix))]
mod child_reap_e2e {
    use super::*;
    use std::time::{Duration, Instant};

    fn make_output_target() -> SharedOutputTarget {
        let (tx, _rx) = mpsc::channel(1);
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
    }

    /// Poll `/proc/<pid>` until the pid is gone entirely, with a
    /// deadline. Returns `true` if the pid was still present (i.e. not
    /// reaped) when the deadline elapsed.
    fn pid_left_unreaped(pid: u32, deadline: Duration) -> bool {
        let start = Instant::now();
        loop {
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                return false;
            }
            if start.elapsed() >= deadline {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Spawn a real PTY + shell and build a `MuxPane` from the
    /// resulting `SpawnedPty`, exactly as `register_pane_and_start_reader`
    /// does. Returns `None` when the environment cannot open a PTY
    /// (SPEC A4: skip cleanly) rather than failing the test.
    fn spawn_pane_for_reap_test(id: PaneId) -> Option<(MuxPane, u32)> {
        let spawned = spawn_pty(80, 24, "test-pane").ok()?;
        let pid = spawned.child.process_id()?;
        let target = make_output_target();
        let pane = MuxPane::new(
            id,
            80,
            24,
            target,
            spawned.writer,
            spawned.master,
            Some(spawned.child),
        );
        Some((pane, pid))
    }

    /// AC-1 (TS-7): the child handle returned by the real spawn call is
    /// carried all the way into the pane and reaped on teardown. A
    /// child left un-stored (discarded at the spawn site, the pre-fix
    /// behavior) would never be reaped here and this assertion would
    /// time out with the pid still present.
    #[test]
    fn spawned_child_is_stored_and_reaped_on_mark_exited() {
        let Some((mut pane, pid)) = spawn_pane_for_reap_test(1) else {
            eprintln!("skipping: environment cannot open a PTY");
            return;
        };
        pane.mark_exited();
        assert!(
            !pid_left_unreaped(pid, Duration::from_secs(3)),
            "pid {pid} must not remain unreaped after mark_exited"
        );
    }

    /// AC-3/AC-7 (TS-8): the same teardown path used by every
    /// production caller (`mark_exited`) reaps the child — pins the
    /// shared gate all four teardown call sites converge on (D4).
    #[test]
    fn mark_exited_reaps_child_after_the_shell_has_run_briefly() {
        let Some((mut pane, pid)) = spawn_pane_for_reap_test(2) else {
            eprintln!("skipping: environment cannot open a PTY");
            return;
        };
        // Let the shell actually run briefly before tearing it down,
        // closer to a real teardown than an immediate mark_exited.
        std::thread::sleep(Duration::from_millis(50));
        pane.mark_exited();
        assert!(!pid_left_unreaped(pid, Duration::from_secs(3)));
    }

    /// AC-7 (TS-9): repeatedly opening and tearing down panes leaves no
    /// test-owned pid behind unreaped — a regression guard for the
    /// zombie-accumulation bug this feature exists to fix.
    #[test]
    fn repeated_open_close_leaves_no_unreaped_children() {
        const N: usize = 5;
        let mut pids = Vec::with_capacity(N);
        for i in 0..N {
            let Some((mut pane, pid)) = spawn_pane_for_reap_test(10 + i as PaneId) else {
                eprintln!("skipping: environment cannot open a PTY");
                return;
            };
            pane.mark_exited();
            pids.push(pid);
        }
        for pid in pids {
            assert!(
                !pid_left_unreaped(pid, Duration::from_secs(3)),
                "pid {pid} must not remain unreaped"
            );
        }
    }
}

// ── mux-suppressed-output-round2-fixes task0004 (FR8, finding
//    `3eccc254dd278b33`): a tail the snapshot already carried is not
//    re-sent ────────────────────────────────────────────────────────────

mod fr8_snapshot_tail {
    use super::*;
    use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
    use crate::mux::session::pane::{AnyPermit, ChunkKind, ResumeOutcome, resume_pane_with_permit};
    use crate::mux::snapshot_tail::trailing_construct_bytes;
    use term_core::terminal_core::TerminalCore;

    const ESC: u8 = 0x1b;
    const COLS: u16 = 80;
    const ROWS: u16 = 24;

    /// Answers `OSC 11 ; ?` with a fixed color report and nothing else, so a
    /// test can count color-query answers without the gui theme.
    struct FixedColorResponder;

    impl term_core::OscResponder for FixedColorResponder {
        fn respond(
            &self,
            code: u16,
            payload: &str,
            _terminator: term_core::OscTerminator,
        ) -> Vec<Vec<u8>> {
            if code == 11 && payload == "?" {
                vec![b"\x1b]11;rgb:1111/2222/3333\x07".to_vec()]
            } else {
                Vec::new()
            }
        }
    }

    fn new_core() -> TerminalCore {
        let mut core = TerminalCore::new(COLS, ROWS, 10_000);
        core.osc_responder = Some(Box::new(FixedColorResponder));
        core
    }

    /// The client's view of a delivered snapshot chunk: replayed through
    /// `reset_and_replay_segments`, its responses discarded (the client never
    /// answers a snapshot's own queries).
    fn apply_snapshot(core: &mut TerminalCore, chunk: &PtyOutputChunk) {
        assert_eq!(chunk.kind, ChunkKind::Snapshot);
        let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&chunk.data);
        let replay: Vec<ReplaySegment> = segments
            .iter()
            .map(|s| ReplaySegment {
                offset: s.offset,
                cols: s.cols,
                rows: s.rows,
            })
            .collect();
        core.reset_and_replay_segments(content, &replay);
        let _ = core.take_response();
    }

    /// Spawn `pty_reader_loop` on a background thread against `pane`, fed
    /// `chunks`, WITHOUT touching the pane's output target (unlike
    /// `spawn_reader_with_chunks`, which forces `Connected`).
    fn spawn_reader_keeping_target(
        pane: &MuxPane,
        chunks: Vec<Vec<u8>>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn({
            let output_target = pane.output_target.clone();
            let shadow_parser = pane.shadow_parser.clone();
            let cwd = pane.cwd.clone();
            let title = pane.title.clone();
            let title_sender = pane.title_sender.clone();
            let notification_sender = pane.notification_sender.clone();
            let agent_status_report_sender = pane.agent_status_report_sender.clone();
            let raw_passthrough = pane.raw_passthrough.clone();
            let passthrough_scanner = pane.passthrough_scanner.clone();
            let scrollback = pane.scrollback.clone();
            let dims = pane.dims.clone();
            let output_capture = pane.output_capture.clone();
            let pane_id = pane.id;
            move || {
                pty_reader_loop(
                    pane_id,
                    Box::new(ScriptedReader::new(chunks)),
                    output_target,
                    shadow_parser,
                    cwd,
                    title,
                    title_sender,
                    notification_sender,
                    agent_status_report_sender,
                    raw_passthrough,
                    passthrough_scanner,
                    scrollback,
                    dims,
                    Arc::new(StdMutex::new(None)),
                    output_capture,
                );
            }
        })
    }

    /// What the owner's channel received during a visibility restore that
    /// landed between the first chunk's capture step and its forward
    /// decision.
    struct RestoreRun {
        /// Everything received, in order (snapshot first, EOF last).
        received: Vec<PtyOutputChunk>,
    }

    impl RestoreRun {
        fn snapshot(&self) -> &PtyOutputChunk {
            &self.received[0]
        }

        /// Non-snapshot chunks after the snapshot, EOF (empty) excluded.
        fn forwarded(&self) -> Vec<Vec<u8>> {
            self.received[1..]
                .iter()
                .filter(|c| !c.data.is_empty())
                .map(|c| c.data.clone())
                .collect()
        }
    }

    /// Run the production visibility restore (`resume_pane_with_permit`) of
    /// a main-screen pane while the reader is paused between `chunks[0]`'s
    /// capture step and its forward decision (P2), then let the reader run
    /// to EOF. `ring` replaces the pane's scrollback ring when given.
    fn run_visibility_restore(
        pane_id: PaneId,
        ring: Option<ScrollbackRingBuffer>,
        chunks: Vec<Vec<u8>>,
    ) -> RestoreRun {
        let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
        let output_target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                reason: DetachReason::HiddenByVisibility,
                owner: Some(tx.clone()),
            }));
        let pane = MuxPane::new_test(pane_id, COLS, ROWS, output_target);
        if let Some(ring) = ring {
            *pane.scrollback.lock().unwrap() = ring;
        }
        let (arrived_rx, release_tx) = pane.output_capture.p2.arm();
        let handle = spawn_reader_keeping_target(&pane, chunks);
        arrived_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the reader must reach P2 on the first chunk");

        let permit = tx.try_reserve().expect("capacity for the resume permit");
        let outcome = resume_pane_with_permit(&pane, &tx, AnyPermit::Borrowed(permit), 10_000);
        assert!(matches!(outcome, ResumeOutcome::Resumed));

        release_tx.send(()).unwrap();
        handle.join().unwrap();

        let mut received = Vec::new();
        while let Ok(c) = rx.try_recv() {
            received.push(c);
        }
        assert!(
            received.len() >= 2,
            "at least the snapshot and EOF must have been delivered"
        );
        assert!(
            received.last().unwrap().data.is_empty(),
            "the last chunk is the EOF marker"
        );
        RestoreRun { received }
    }

    fn assert_display_matches(client: &TerminalCore, reference: &TerminalCore, what: &str) {
        for r in 0..ROWS {
            let got = client.get_line_text(r);
            assert_eq!(
                got.trim_end(),
                reference.get_line_text(r).trim_end(),
                "{what}: row {r} mismatch"
            );
            assert!(
                !got.contains('\u{fffd}'),
                "{what}: row {r} shows a replacement character: {got:?}"
            );
        }
        assert_eq!(
            client.get_cursor_row(),
            reference.get_cursor_row(),
            "{what}: cursor row"
        );
        assert_eq!(
            client.get_cursor_col(),
            reference.get_cursor_col(),
            "{what}: cursor col"
        );
    }

    struct Split {
        name: &'static str,
        first: Vec<u8>,
        second: Vec<u8>,
        /// A character that must not appear in any client row (the
        /// designator `(` the pre-fix re-send displays).
        forbidden: Option<char>,
    }

    /// The three splits of AC-4: mid UTF-8, right after `ESC (`, and inside
    /// an incomplete CSI.
    fn splits() -> Vec<Split> {
        vec![
            Split {
                name: "mid utf-8",
                first: [b"abc".as_slice(), &[0xe4, 0xb8]].concat(),
                second: [[0xad].as_slice(), b"def\r\n"].concat(),
                forbidden: None,
            },
            Split {
                name: "right after ESC (",
                first: [b"abc".as_slice(), &[ESC, b'(']].concat(),
                second: b"0lqk\x1b(B\r\nZ".to_vec(),
                forbidden: Some('('),
            },
            Split {
                name: "inside an incomplete csi",
                first: [b"abc".as_slice(), &[ESC, b'[', b'3']].concat(),
                second: b"1mred\x1b[0m\r\n".to_vec(),
                forbidden: None,
            },
        ]
    }

    fn check_client_against_reference(run: &RestoreRun, split: &Split, what: &str) {
        let mut client = new_core();
        apply_snapshot(&mut client, run.snapshot());
        for data in run.forwarded() {
            client.process_pty_data_fully(&data);
        }
        let mut reference = new_core();
        reference.process_pty_data_fully(&split.first);
        reference.process_pty_data_fully(&split.second);
        assert_display_matches(&client, &reference, what);
        if let Some(forbidden) = split.forbidden {
            for r in 0..ROWS {
                assert!(
                    !client.get_line_text(r).contains(forbidden),
                    "{what}: row {r} must not display {forbidden:?}: {:?}",
                    client.get_line_text(r)
                );
            }
        }
    }

    /// AC-4 (registry, finding `3eccc254dd278b33`): visibility restore of a
    /// main-screen pane, ring not wrapped, three splits. The first half is
    /// suppressed. For every split the client's display, cursor and parsing of
    /// the following chunk match a reference fed the raw stream, and the tail
    /// the snapshot already carries is not re-sent (the second half is the
    /// very next delivery). The `ESC (` split fails the display comparison on
    /// the pre-fix code: the re-sent `ESC (` makes the client take the second
    /// ESC as the designator and display a literal `(`, and the next chunk's
    /// first byte is printed instead of selecting the charset.
    #[test]
    fn round2_3eccc254_visibility_restore_does_not_resend_a_tail_the_snapshot_carried() {
        let runs: Vec<(Split, RestoreRun)> = splits()
            .into_iter()
            .enumerate()
            .map(|(i, split)| {
                let run = run_visibility_restore(
                    200 + i as PaneId,
                    None,
                    vec![split.first.clone(), split.second.clone()],
                );
                (split, run)
            })
            .collect();
        for (split, run) in &runs {
            check_client_against_reference(run, split, split.name);
        }
        for (split, run) in &runs {
            assert_eq!(
                run.forwarded(),
                vec![split.second.clone()],
                "{}: nothing may be re-sent between the snapshot and the next chunk",
                split.name
            );
        }
    }

    /// AC-4 companion: one split of the registry test's body, so a failure
    /// names its split. The client comparison runs first, then the
    /// "nothing re-sent" check.
    fn check_split(pane_id: PaneId, index: usize) {
        let split = splits().remove(index);
        let run = run_visibility_restore(
            pane_id,
            None,
            vec![split.first.clone(), split.second.clone()],
        );
        check_client_against_reference(&run, &split, split.name);
        assert_eq!(
            run.forwarded(),
            vec![split.second.clone()],
            "{}: nothing may be re-sent between the snapshot and the next chunk",
            split.name
        );
    }

    #[test]
    fn visibility_restore_of_a_cut_utf8_tail_sends_nothing_between_snapshot_and_next_chunk() {
        check_split(210, 0);
    }

    #[test]
    fn visibility_restore_after_esc_paren_sends_nothing_and_the_next_chunk_selects_the_charset() {
        check_split(211, 1);
        let split = splits().remove(1);
        let run =
            run_visibility_restore(213, None, vec![split.first.clone(), split.second.clone()]);
        let mut client = new_core();
        apply_snapshot(&mut client, run.snapshot());
        for data in run.forwarded() {
            client.process_pty_data_fully(&data);
        }
        assert!(
            client.get_line_text(0).contains('\u{250c}'),
            "the first byte of the next chunk must select DEC line drawing: {:?}",
            client.get_line_text(0)
        );
    }

    #[test]
    fn visibility_restore_of_a_cut_csi_tail_sends_nothing_between_snapshot_and_next_chunk() {
        check_split(212, 2);
    }

    /// AC-4, second ring shape: ring wrapped with an EMPTY screen dump. The
    /// shadow parser's dump is never empty in production, so this payload
    /// shape is driven through the snapshot assembly function and the
    /// replacement builder directly (task plan Test Notes), with the same
    /// client/reference comparison.
    #[test]
    fn wrapped_ring_with_an_empty_dump_does_not_resend_a_tail_the_snapshot_carried() {
        use crate::mux::ipc::pty_spawn::suppressed_output::{
            SuppressedReplacementRequest, build_suppressed_replacement_for,
        };
        use crate::mux::snapshot_bytes::build_resume_snapshot_bytes_for_ring;

        // Bytes that only move the cursor to column 0, so the ring may lose
        // its head without changing the visible state.
        let leading = vec![b'\r'; 20];
        for split in splits() {
            let first = [leading.as_slice(), &split.first].concat();
            let mut ring = ScrollbackRingBuffer::new(16);
            ring.write(&first);
            let (ring_bytes, ring_segments, wrapped) = ring.read_segments_with_wrap_state();
            assert!(
                wrapped,
                "{}: test prerequisite: the ring wrapped",
                split.name
            );
            let (payload, segments) = build_resume_snapshot_bytes_for_ring(
                &ring_bytes,
                &ring_segments,
                &[],
                false,
                true,
                (COLS, ROWS),
                10_000,
            );
            assert!(
                payload.ends_with(&split.first[3..]),
                "{}: the payload still ends in the ring's cut construct",
                split.name
            );
            let construct = trailing_construct_bytes(&payload);
            assert!(
                construct.is_some(),
                "{}: a construct is decided",
                split.name
            );

            let replacement = build_suppressed_replacement_for(&SuppressedReplacementRequest {
                chunk: &first,
                ring_written_ranges: &[0..first.len()],
                pending_after: &[],
                window: &[],
                snapshot_trailing_construct: construct.as_deref(),
            });
            assert!(
                replacement.is_empty(),
                "{}: nothing is re-sent, got {replacement:?}",
                split.name
            );

            let encoded = crate::mux::session::pane::encode_snapshot_segments(&payload, &segments);
            let chunk = PtyOutputChunk::snapshot(1, encoded);
            let mut client = new_core();
            apply_snapshot(&mut client, &chunk);
            client.process_pty_data_fully(&replacement);
            client.process_pty_data_fully(&split.second);
            let mut reference = new_core();
            reference.process_pty_data_fully(&first);
            reference.process_pty_data_fully(&split.second);
            assert_display_matches(&client, &reference, split.name);
        }
    }

    // ---- AC-5: a query coexisting with the tail ----

    /// One tail kind: the bytes the suppressed chunk ends in, what the
    /// replacement must carry for it, and the next chunk.
    struct TailKind {
        name: &'static str,
        tail: Vec<u8>,
        /// The designator case needs one filler byte first (the client's
        /// pending designator slot absorbs it).
        needs_filler: bool,
        second: Vec<u8>,
    }

    fn tail_kinds() -> Vec<TailKind> {
        vec![
            TailKind {
                name: "cut utf-8",
                tail: vec![0xe4, 0xb8],
                needs_filler: false,
                second: [[0xad].as_slice(), b"def\r\n"].concat(),
            },
            TailKind {
                name: "ESC (",
                tail: vec![ESC, b'('],
                needs_filler: true,
                second: b"0lqk\x1b(B\r\nZ".to_vec(),
            },
            TailKind {
                name: "incomplete csi",
                tail: vec![ESC, b'[', b'3'],
                needs_filler: false,
                second: b"1mred\x1b[0m\r\n".to_vec(),
            },
        ]
    }

    /// `prefix` (holding one query) + `abc` + each tail kind, restored
    /// through the reader. `item` is the query as the replacement carries it.
    fn check_query_with_each_tail(pane_id_base: PaneId, prefix: &[u8], item: &[u8]) {
        for (i, kind) in tail_kinds().into_iter().enumerate() {
            let first = [prefix, b"abc".as_slice(), &kind.tail].concat();
            let run = run_visibility_restore(
                pane_id_base + i as PaneId,
                None,
                vec![first.clone(), kind.second.clone()],
            );
            let mut expected_replacement = Vec::new();
            if kind.needs_filler {
                expected_replacement.push(b'B');
            }
            expected_replacement.extend_from_slice(item);
            expected_replacement.extend_from_slice(&kind.tail);
            assert_eq!(
                run.forwarded(),
                vec![expected_replacement, kind.second.clone()],
                "{}: the query and the tail, then the next chunk",
                kind.name
            );

            let mut client = new_core();
            apply_snapshot(&mut client, run.snapshot());
            let mut client_responses = Vec::new();
            for data in run.forwarded() {
                client.process_pty_data_fully(&data);
                client_responses.extend(client.take_response());
            }
            let mut reference = new_core();
            let mut reference_responses = Vec::new();
            for data in [&first, &kind.second] {
                reference.process_pty_data_fully(data);
                reference_responses.extend(reference.take_response());
            }
            assert!(
                !reference_responses.is_empty(),
                "{}: test prerequisite: the query is answered",
                kind.name
            );
            assert_eq!(
                client_responses, reference_responses,
                "{}: the query must be answered exactly once, as on the reference",
                kind.name
            );
            assert_display_matches(&client, &reference, kind.name);
        }
    }

    /// AC-5: an alternate-screen color query (absent from the snapshot) and
    /// each of the three tail kinds in one suppressed chunk.
    #[test]
    fn suppressed_chunk_with_a_color_query_and_each_tail_kind_answers_once_and_parses_on() {
        check_query_with_each_tail(
            220,
            b"\x1b[?1049h\x1b]11;?\x07\x1b[?1049l",
            b"\x1b]11;?\x07",
        );
    }

    /// AC-5: a CSI device query and each of the three tail kinds in one
    /// suppressed chunk.
    #[test]
    fn suppressed_chunk_with_a_csi_query_and_each_tail_kind_answers_once_and_parses_on() {
        check_query_with_each_tail(230, b"\x1b[c", b"\x1b[c");
    }

    // ---- AC-6: shapes that keep the pre-feature re-send ----

    /// AC-6 (as-03): a wrapped ring gets a dump block after the ring, so the
    /// snapshot does not end in the cut construct; nothing is recorded and
    /// the tail is re-sent as before.
    #[test]
    fn visibility_restore_of_a_wrapped_ring_with_a_dump_block_still_resends_the_tail() {
        let first = [b"visible text\r\n".repeat(5), vec![ESC, b'(']].concat();
        let second = b"0lqk\x1b(B\r\nZ".to_vec();
        let run = run_visibility_restore(
            240,
            Some(ScrollbackRingBuffer::new(32)),
            vec![first, second.clone()],
        );
        assert_eq!(
            run.forwarded(),
            vec![vec![ESC, b'('], second],
            "the dump block ends the snapshot, so the tail is re-sent as before"
        );
    }

    /// Run the reader over `chunks` for a `Connected(dest)` pane after
    /// `record` has recorded boundaries, returning what `dest` received
    /// (EOF excluded).
    fn run_reader_with_records(
        pane_id: PaneId,
        chunks: Vec<Vec<u8>>,
        record: impl FnOnce(&OutputCapture, &mpsc::Sender<PtyOutputChunk>),
    ) -> Vec<Vec<u8>> {
        let (dest, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
        let output_target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(dest.clone())));
        let pane = MuxPane::new_test(pane_id, COLS, ROWS, output_target);
        record(&pane.output_capture, &dest);
        spawn_reader_keeping_target(&pane, chunks).join().unwrap();
        let mut out = Vec::new();
        while let Ok(c) = rx.try_recv() {
            if !c.data.is_empty() {
                out.push(c.data);
            }
        }
        out
    }

    use crate::mux::session::pane::OutputCapture;

    /// AC-6: with two destinations, each uses only its own record.
    #[test]
    fn a_suppressed_chunk_uses_only_the_construct_recorded_for_its_own_destination() {
        let esc_paren = vec![ESC, b'('];
        let first = [b"abc".as_slice(), &esc_paren].concat();

        // The construct is recorded for ANOTHER destination only: this
        // destination's own record has none, so the tail is re-sent.
        let (other, _other_rx) = mpsc::channel::<PtyOutputChunk>(4);
        let got = run_reader_with_records(250, vec![first.clone()], |capture, dest| {
            capture.record_boundary_with_construct(&other, 1, Some(esc_paren.clone()));
            capture.record_boundary(dest, 1);
        });
        assert_eq!(got, vec![esc_paren.clone()]);

        // Its own record carries the construct: the tail is omitted, and the
        // other destination's record is not consulted.
        let (other, _other_rx) = mpsc::channel::<PtyOutputChunk>(4);
        let got = run_reader_with_records(251, vec![first.clone()], |capture, dest| {
            capture.record_boundary(&other, 1);
            capture.record_boundary_with_construct(dest, 1, Some(esc_paren.clone()));
        });
        assert!(got.is_empty(), "nothing to send, got {got:?}");
    }

    /// AC-6: the construct applies only to the chunk numbered exactly at the
    /// recorded boundary; an earlier covered chunk re-sends its tail.
    #[test]
    fn the_construct_applies_only_to_the_chunk_at_the_recorded_boundary() {
        let esc_paren = vec![ESC, b'('];
        let chunk1 = [b"abc".as_slice(), &esc_paren].concat();
        let chunk2 = [b"xyz".as_slice(), &esc_paren].concat();
        let got = run_reader_with_records(252, vec![chunk1, chunk2], |capture, dest| {
            capture.record_boundary_with_construct(dest, 2, Some(esc_paren.clone()));
        });
        assert_eq!(
            got,
            vec![esc_paren.clone()],
            "chunk 1 (below the boundary) re-sends its tail; chunk 2 (at the \
             boundary) uses the construct and sends nothing"
        );
    }
}
