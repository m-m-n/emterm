use super::*;
use crate::viewer_kinds::REPLAYABLE_VIEWER_KINDS;

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

/// task0004 round-4 rework (D1'): there is no `resize` OSC 777 kind any
/// more — a marker-SHAPED byte sequence is just an ordinary, unrecognized
/// OSC 777 kind (like `status-bar`) and is KEPT, byte-for-byte, by both
/// the snapshot-time strip and the write-path strip (which are now the
/// SAME function — see [`strip_pty_output_for_scrollback_write`]'s doc
/// comment). This is intentional: since dimensions never travel in the
/// byte stream at all any more, there is nothing security-sensitive
/// about this sequence surviving — it carries no authority, unlike the
/// pre-D1' design where a surviving marker WAS the dimension change.
#[test]
fn marker_shaped_osc777_bytes_are_kept_identically_by_both_strip_paths() {
    let marker_shaped = b"\x1b]777;emterm;resize;120;48\x07";
    let mut input = b"before".to_vec();
    input.extend_from_slice(marker_shaped);
    input.extend_from_slice(b"after");

    let snapshot_out = strip_replayable_rich_content(&input);
    let write_out = strip_pty_output_for_scrollback_write(&input);
    assert_eq!(
        snapshot_out, input,
        "marker-shaped bytes are an ordinary, unrecognized OSC 777 kind \
         and must be preserved byte-for-byte"
    );
    assert_eq!(
        write_out, input,
        "the write-path strip must behave identically — there is no \
         more resize-specific stripping"
    );
}

/// The write-path function and the snapshot-time function behave
/// IDENTICALLY for every input (task0004 round-4 rework D1': they are
/// now literally the same implementation) — viewer launches, fold
/// marks, device queries, and plain text all strip the same way.
#[test]
fn strip_pty_output_for_scrollback_write_matches_snapshot_strip_for_everything() {
    let input =
        b"$ ls\r\n\x1b]777;emterm;markdown;begin\x07\x1b]777;emterm;fold;start;1\x07done\x1b[c";
    assert_eq!(
        strip_pty_output_for_scrollback_write(input),
        strip_replayable_rich_content(input)
    );
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
    let (out, _segments) = build_snapshot_bytes(scrollback, &[], b"", false, (80, 24));
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

// ── task0004 round-4 rework (D1'): strip_rich_content_and_remap keeps
// structural dimension segment offsets valid after the strip removes
// bytes ahead of them ──────────────────────────────────────────────

/// No stripping occurs: every watch offset maps to itself.
#[test]
fn remap_identity_when_nothing_is_stripped() {
    let input = b"plain text, nothing removable here";
    let watch = [0usize, 5, input.len()];
    let (out, remapped) = strip_rich_content_and_remap(input, &watch);
    assert_eq!(out, input);
    assert_eq!(remapped, vec![0, 5, input.len()]);
}

/// A watch offset AFTER a stripped sequence shifts back by exactly the
/// stripped sequence's length.
#[test]
fn remap_shifts_offset_after_a_stripped_sequence() {
    let mut input = b"before".to_vec();
    let viewer_launch = b"\x1b]777;emterm;markdown;begin\x07";
    input.extend_from_slice(viewer_launch);
    input.extend_from_slice(b"after");
    let after_offset = input.len() - b"after".len();
    let (out, remapped) = strip_rich_content_and_remap(&input, &[after_offset]);
    assert_eq!(out, b"beforeafter");
    assert_eq!(
        remapped,
        vec![b"before".len()],
        "offset must land right where 'after' starts in the stripped output"
    );
}

/// A watch offset falling STRICTLY INSIDE a stripped sequence maps to
/// the output position immediately after the content that preceded it
/// (the removed span contributes nothing at any position within it).
#[test]
fn remap_offset_inside_a_stripped_sequence_maps_past_it() {
    let mut input = b"before".to_vec();
    let viewer_launch = b"\x1b]777;emterm;markdown;begin\x07";
    input.extend_from_slice(viewer_launch);
    input.extend_from_slice(b"after");
    // An offset landing mid-way through the viewer launch sequence.
    let mid_launch_offset = b"before".len() + 5;
    let (out, remapped) = strip_rich_content_and_remap(&input, &[mid_launch_offset]);
    assert_eq!(out, b"beforeafter");
    assert_eq!(remapped, vec![b"before".len()]);
}

/// A watch offset exactly at `bytes.len()` (the end) maps to the final
/// output length — the loop's exit condition must not skip this case.
#[test]
fn remap_offset_at_end_of_input_maps_to_end_of_output() {
    let mut input = b"before".to_vec();
    input.extend_from_slice(b"\x1b]777;emterm;markdown;begin\x07");
    let (out, remapped) = strip_rich_content_and_remap(&input, &[input.len()]);
    assert_eq!(out, b"before");
    assert_eq!(remapped, vec![out.len()]);
}

/// Multiple watch offsets in one call, spanning before / inside / after
/// TWO stripped sequences, each remapped correctly in a single pass.
#[test]
fn remap_multiple_offsets_across_multiple_stripped_sequences() {
    let mut input = Vec::new();
    input.extend_from_slice(b"AAA"); // [0, 3)
    let launch1 = b"\x1b]777;emterm;markdown;begin\x07";
    input.extend_from_slice(launch1); // [3, 3+launch1.len())
    input.extend_from_slice(b"BBB"); // after launch1
    let launch2 = b"\x1b_Gi=1,a=T;PAYLOAD\x1b\\"; // Kitty APC
    input.extend_from_slice(launch2);
    input.extend_from_slice(b"CCC");

    let offset_in_aaa = 1usize;
    let offset_in_launch1 = 3 + 2;
    let offset_in_bbb = 3 + launch1.len() + 1;
    let offset_in_launch2 = 3 + launch1.len() + 3 + 2;
    let offset_in_ccc = 3 + launch1.len() + 3 + launch2.len() + 1;

    let watch = [
        offset_in_aaa,
        offset_in_launch1,
        offset_in_bbb,
        offset_in_launch2,
        offset_in_ccc,
    ];
    let (out, remapped) = strip_rich_content_and_remap(&input, &watch);
    assert_eq!(out, b"AAABBBCCC");
    assert_eq!(
        remapped,
        vec![
            1,         // inside AAA: unaffected
            3,         // inside launch1: maps past "AAA"
            3 + 1,     // inside BBB: "AAA" + 1 byte into BBB
            3 + 3,     // inside launch2: maps past "AAABBB"
            3 + 3 + 1, // inside CCC: "AAABBB" + 1 byte into CCC
        ]
    );
}

// ── shared OSC identification on the strip side (round-2 task0001, FR7) ──

/// OSC bodies the strip must remove although they are not the canonical
/// `777;emterm;<kind>;…` / `9999;emterm-md…` spelling: a leading-zero number
/// or non-digit bytes before the first `;` still reach the client's parser as
/// the same OSC number and data (NFR1 exception).
const NON_CANONICAL_STRIPPED_BODIES: &[&[u8]] = &[
    b"0777;emterm;markdown;begin",
    b"09999;emterm-md;begin",
    b"777emterm;;markdown;begin",
    b"0777;emterm;image;begin",
    b"0777;emterm;agent-status;v=1;state=idle",
    b"0000000000777;emterm;json;begin",
    b"0777;emterm;yaml",
    b"0777;emterm;html;begin",
    b"09999;emterm-md",
];

/// OSC bodies the strip must keep byte-for-byte.
const KEPT_BODIES: &[&[u8]] = &[
    b"777;emterm;fold;start;1",
    b"0777;emterm;fold;start;1",
    b"9999;emterm-mux;state;1",
    b"09999;emterm-mux;state;1",
    b"9999;emterm-mdx",
    b"777;emterm;status-bar;line",
    b"0777;emterm;status-bar;line",
    b"777;other;x",
    b"778;emterm;markdown;begin",
    // u16 overflow (as-06): no route can be established, so nothing is
    // identified and nothing is stripped.
    b"65536;emterm;markdown;begin",
    b"70000;emterm;markdown;begin",
    b"99999;emterm-md;begin",
];

fn osc(body: &[u8], terminator: &[u8]) -> Vec<u8> {
    let mut out = b"\x1b]".to_vec();
    out.extend_from_slice(body);
    out.extend_from_slice(terminator);
    out
}

/// AC-3: the ring-write strip removes every non-canonical spelling of a viewer
/// launch, Markdown launch or agent-status report, with either terminator.
#[test]
fn ring_write_strip_removes_leading_zero_and_non_digit_prefixed_launches() {
    for body in NON_CANONICAL_STRIPPED_BODIES {
        for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
            let mut input = b"before".to_vec();
            input.extend_from_slice(&osc(body, terminator));
            input.extend_from_slice(b"after");
            assert_eq!(
                strip_pty_output_for_scrollback_write(&input),
                b"beforeafter",
                "body {:?} must be stripped on ring write",
                String::from_utf8_lossy(body)
            );
        }
    }
}

/// AC-3: the snapshot strip removes the same set.
#[test]
fn snapshot_strip_removes_leading_zero_and_non_digit_prefixed_launches() {
    for body in NON_CANONICAL_STRIPPED_BODIES {
        for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
            let mut input = b"before".to_vec();
            input.extend_from_slice(&osc(body, terminator));
            input.extend_from_slice(b"after");
            assert_eq!(
                strip_replayable_rich_content(&input),
                b"beforeafter",
                "body {:?} must be stripped on snapshot assembly",
                String::from_utf8_lossy(body)
            );
        }
    }
}

/// AC-3: the strip-and-remap entry point (used by snapshot byte assembly)
/// removes the same set and keeps a watch offset after it aligned.
#[test]
fn strip_and_remap_removes_leading_zero_and_non_digit_prefixed_launches() {
    for body in NON_CANONICAL_STRIPPED_BODIES {
        let mut input = b"before".to_vec();
        input.extend_from_slice(&osc(body, b"\x07"));
        let after_offset = input.len();
        input.extend_from_slice(b"after");
        let (out, remapped) = strip_rich_content_and_remap(&input, &[after_offset]);
        assert_eq!(
            out,
            b"beforeafter",
            "body {:?}",
            String::from_utf8_lossy(body)
        );
        assert_eq!(remapped, vec![b"before".len()]);
    }
}

/// AC-3: a full `build_snapshot_bytes` product built from a scrollback holding
/// a leading-zero viewer launch contains no trace of the launch.
#[test]
fn build_snapshot_bytes_funnel_strips_leading_zero_viewer_launch() {
    use crate::mux::snapshot_bytes::build_snapshot_bytes;
    let mut scrollback = b"prompt$ ".to_vec();
    scrollback.extend_from_slice(&osc(b"0777;emterm;markdown;begin", b"\x07"));
    scrollback.extend_from_slice(&osc(b"09999;emterm-md;begin", b"\x1b\\"));
    scrollback.extend_from_slice(b"done");
    let (out, _segments) = build_snapshot_bytes(&scrollback, &[], b"", false, (80, 24));
    assert!(
        !out.windows(b"emterm".len()).any(|w| w == b"emterm"),
        "snapshot must not carry a viewer launch: {out:?}"
    );
    assert!(out.windows(b"prompt".len()).any(|w| w == b"prompt"));
    assert!(out.windows(b"done".len()).any(|w| w == b"done"));
}

/// AC-3: fold marks, mux control, other kinds and overflowed numbers are kept
/// by every strip entry point.
#[test]
fn every_strip_entry_point_keeps_non_launch_and_overflowed_osc_bodies() {
    for body in KEPT_BODIES {
        for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
            let mut input = b"L".to_vec();
            input.extend_from_slice(&osc(body, terminator));
            input.extend_from_slice(b"R");
            let label = String::from_utf8_lossy(body);
            assert_eq!(
                strip_pty_output_for_scrollback_write(&input),
                input,
                "body {label:?} must be kept on ring write"
            );
            assert_eq!(
                strip_replayable_rich_content(&input),
                input,
                "body {label:?} must be kept on snapshot assembly"
            );
            let (out, _) = strip_rich_content_and_remap(&input, &[]);
            assert_eq!(out, input, "body {label:?} must be kept by strip-and-remap");
        }
    }
}

// ── designator-aware strip (mux-suppressed-output-round3-fixes task0002,
//    FR2 / FR3) ──────────────────────────────────────────────────────────
//
// term_core consumes the byte after `ESC (` / `ESC )` as the charset
// designator, even when it is an ESC, so that byte never opens an OSC, APC,
// DCS or CSI. The strip follows the same transition in every entry point.

/// Runs in which the byte after `ESC (` / `ESC )` is an ESC: nothing in them
/// is a strip target for the client, so every entry point keeps every byte.
const DESIGNATOR_ESC_KEPT_RUNS: &[&[u8]] = &[
    b"\x1b(\x1b]0777;emterm;markdown;begin;id=x\x07X",
    b"\x1b)\x1b[6n",
    b"\x1b(\x1b]777;emterm;markdown;x\x07",
    b"\x1b)\x1b]777;emterm;agent-status;v=1;state=idle\x07",
    b"\x1b(\x1b_Gi=1,a=T;PAYLOAD\x1b\\",
    b"\x1b)\x1bPq#0;2;0;0;0\x1b\\",
    b"\x1b(\x1b]9999;emterm-md;begin\x1b\\",
    b"a\x1b(\x1b[5nb",
];

/// Every entry point (snapshot, ring write, strip-and-remap, and the
/// state-taking forms with the flag clear) keeps a designator ESC's run.
#[test]
fn every_strip_entry_point_keeps_a_designator_esc_and_what_follows_it() {
    for input in DESIGNATOR_ESC_KEPT_RUNS {
        let label = String::from_utf8_lossy(input);
        assert_eq!(
            strip_replayable_rich_content(input),
            *input,
            "{label:?}: snapshot strip"
        );
        assert_eq!(
            strip_pty_output_for_scrollback_write(input),
            *input,
            "{label:?}: ring-write strip"
        );
        assert_eq!(
            strip_rich_content_and_remap(input, &[]).0,
            *input,
            "{label:?}: strip-and-remap"
        );
        assert_eq!(
            strip_pty_output_for_scrollback_write_with_designator(input, false),
            *input,
            "{label:?}: state-taking write form, flag clear"
        );
        assert_eq!(
            strip_rich_content_and_remap_with_designator(input, &[], false).0,
            *input,
            "{label:?}: state-taking remap form, flag clear"
        );
    }
}

/// A completed designation (`ESC ( B`) is over: a launch that follows it is a
/// real launch and is still stripped, with either brace and either terminator.
#[test]
fn a_launch_after_a_completed_designator_is_still_stripped() {
    for designation in [&b"\x1b(B"[..], &b"\x1b)0"[..], &b"\x1b(\x1b"[..]] {
        for terminator in [&b"\x07"[..], &b"\x1b\\"[..]] {
            let mut input = b"pre".to_vec();
            input.extend_from_slice(designation);
            input.extend_from_slice(&osc(b"777;emterm;markdown;begin;id=x", terminator));
            input.extend_from_slice(b"X");
            let mut expected = b"pre".to_vec();
            expected.extend_from_slice(designation);
            expected.extend_from_slice(b"X");
            assert_eq!(
                strip_pty_output_for_scrollback_write(&input),
                expected,
                "designation {designation:?}: ring-write strip"
            );
            assert_eq!(
                strip_replayable_rich_content(&input),
                expected,
                "designation {designation:?}: snapshot strip"
            );
        }
    }
}

/// `ESC (` followed by an ESC consumes only ONE ESC: a launch opened by the
/// ESC after it is stripped (`ESC ( ESC ( ESC ]` and `ESC ( ESC ESC ]`).
#[test]
fn only_the_byte_right_after_the_designation_is_consumed() {
    let launch = osc(b"777;emterm;markdown;begin", b"\x07");

    let mut chain = b"\x1b(\x1b(".to_vec();
    chain.extend_from_slice(&launch);
    assert_eq!(
        strip_pty_output_for_scrollback_write(&chain),
        b"\x1b(\x1b(",
        "the second `(` is plain text; the ESC after it opens the launch"
    );

    let mut double_esc = b"\x1b(\x1b".to_vec();
    double_esc.extend_from_slice(&launch);
    assert_eq!(
        strip_pty_output_for_scrollback_write(&double_esc),
        b"\x1b(\x1b",
        "the second ESC is the designator; the third byte starts the launch"
    );
}

/// `ESC (` / `ESC )` at the very end of the input is copied verbatim.
#[test]
fn a_trailing_designation_start_is_copied_verbatim() {
    for input in [
        &b"abc\x1b("[..],
        &b"abc\x1b)"[..],
        &b"\x1b("[..],
        &b"\x1b(\x1b("[..],
    ] {
        assert_eq!(strip_pty_output_for_scrollback_write(input), input);
        assert_eq!(strip_replayable_rich_content(input), input);
    }
}

/// The state-taking form with the flag set copies byte 0 verbatim, even an
/// ESC that would open a launch, and resumes after it.
#[test]
fn the_flag_makes_byte_zero_a_verbatim_designator() {
    let launch = osc(b"777;emterm;markdown;begin;id=x", b"\x07");

    // Byte 0 is an ESC: it does not open the launch that follows it.
    assert_eq!(
        strip_pty_output_for_scrollback_write_with_designator(&launch, true),
        launch,
        "the leading ESC is the designator; the rest is plain text"
    );
    assert_eq!(
        strip_rich_content_and_remap_with_designator(&launch, &[], true).0,
        launch
    );

    // Byte 0 is an ordinary designator; a launch after it is stripped.
    let mut input = b"B".to_vec();
    input.extend_from_slice(&launch);
    input.extend_from_slice(b"Z");
    assert_eq!(
        strip_pty_output_for_scrollback_write_with_designator(&input, true),
        b"BZ"
    );

    // Empty input with the flag set is empty.
    assert!(strip_pty_output_for_scrollback_write_with_designator(b"", true).is_empty());
    assert_eq!(
        strip_rich_content_and_remap_with_designator(b"", &[0, 3], true),
        (Vec::new(), vec![0, 0])
    );

    // A designator byte 0 followed by another `ESC (` run: the new
    // designation is tracked as usual.
    assert_eq!(
        strip_pty_output_for_scrollback_write_with_designator(b"x\x1b(\x1b]0;t\x07", true),
        b"x\x1b(\x1b]0;t\x07"
    );
}

/// With the flag clear the state-taking form equals the existing entry
/// points, on a corpus that mixes strip targets, designations and text.
#[test]
fn the_state_taking_form_with_the_flag_clear_equals_the_existing_entry_points() {
    let launch = osc(b"777;emterm;markdown;begin", b"\x07");
    let mut corpus: Vec<Vec<u8>> = vec![
        b"plain".to_vec(),
        b"a\x1b[6nb".to_vec(),
        b"\x1b(B\x1b)0text".to_vec(),
        b"\x1b_Gi=1;P\x1b\\tail".to_vec(),
        b"\x1bPq#0\x1b\\tail".to_vec(),
        b"\x1b]0;title\x07".to_vec(),
        b"\x1b".to_vec(),
        Vec::new(),
    ];
    corpus.push([b"x".as_slice(), &launch, b"y"].concat());
    corpus.extend(DESIGNATOR_ESC_KEPT_RUNS.iter().map(|r| r.to_vec()));
    for input in &corpus {
        assert_eq!(
            strip_pty_output_for_scrollback_write_with_designator(input, false),
            strip_pty_output_for_scrollback_write(input),
            "{input:?}"
        );
        let watch: Vec<usize> = (0..=input.len()).collect();
        assert_eq!(
            strip_rich_content_and_remap_with_designator(input, &watch, false),
            strip_rich_content_and_remap(input, &watch),
            "{input:?}"
        );
    }
}

/// Remapped watch offsets before, on and after the designator byte map each
/// copied byte one-to-one.
#[test]
fn remap_offsets_around_a_designator_byte_stay_consistent_with_the_output() {
    // `ab`, `ESC ( B`, a stripped launch, `Z`.
    let launch = osc(b"777;emterm;markdown;begin", b"\x07");
    let mut input = b"ab\x1b(B".to_vec();
    input.extend_from_slice(&launch);
    input.extend_from_slice(b"Z");
    let launch_start = 5usize;
    let z = launch_start + launch.len();
    let watch = [0usize, 1, 2, 3, 4, launch_start, launch_start + 3, z, z + 1];
    let (out, remapped) = strip_rich_content_and_remap(&input, &watch);
    assert_eq!(out, b"ab\x1b(BZ");
    assert_eq!(remapped, vec![0, 1, 2, 3, 4, 5, 5, 5, 6]);

    // The designator byte is an ESC: every byte of the run is kept and every
    // offset maps to itself.
    let input = b"\x1b(\x1b]777;emterm;markdown;x\x07";
    let watch: Vec<usize> = (0..=input.len()).collect();
    let (out, remapped) = strip_rich_content_and_remap(input, &watch);
    assert_eq!(out, input.to_vec());
    assert_eq!(remapped, watch);

    // A designation after a stripped launch: offsets on its three bytes shift
    // by exactly the stripped length.
    let mut input = launch.clone();
    input.extend_from_slice(b"\x1b(\x1b]0;t\x07");
    let l = launch.len();
    let (out, remapped) = strip_rich_content_and_remap(&input, &[l, l + 1, l + 2, l + 3]);
    assert_eq!(out, b"\x1b(\x1b]0;t\x07");
    assert_eq!(remapped, vec![0, 1, 2, 3]);

    // The flag set: offsets 0 and 1 stay in place and byte 0 is copied.
    let input = b"\x1b]777;emterm;markdown;x\x07";
    let (out, remapped) = strip_rich_content_and_remap_with_designator(input, &[0, 1, 2], true);
    assert_eq!(out, input.to_vec());
    assert_eq!(remapped, vec![0, 1, 2]);
}

/// Designator chains scan in one bounded pass and keep every byte.
#[test]
fn designator_chains_are_one_pass_and_keep_every_byte() {
    for unit in [&b"\x1b("[..], &b"\x1b(\x1b"[..], &b"\x1b)\x1b)"[..]] {
        let input: Vec<u8> = std::iter::repeat_n(unit, 50_000)
            .flatten()
            .copied()
            .collect();
        let start = std::time::Instant::now();
        let out = strip_pty_output_for_scrollback_write(&input);
        let (remapped_out, remapped) = strip_rich_content_and_remap(&input, &[0, input.len() / 2]);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "chain of {unit:?} took {:?}",
            start.elapsed()
        );
        assert_eq!(out, input);
        assert_eq!(remapped_out, input);
        assert_eq!(remapped, vec![0, input.len() / 2]);
    }
}

// ── state-reporting write-path strip (mux-cut-csi-post-strip-closure, FR2 /
//    FR3 / FR6, D1) ────────────────────────────────────────────────────────
//
// The form reports the CSI sub-state the stream is in after the bytes it
// writes, so the write filter can close a cut and carry the state by what it
// wrote, not by what it was fed.

/// Run `stream` through a fresh term_core and report its CSI sub-state: `None`
/// in ground, otherwise the entry or the parameter state. Only meaningful for a
/// stream that ends in a CSI or in ground (not after a bare `ESC`, not inside a
/// charset designator wait).
fn client_csi_phase(stream: &[u8]) -> Option<CsiPhase> {
    fn shows_m(stream: &[u8]) -> bool {
        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 10);
        core.process_pty_data_fully(&[stream, b"m"].concat());
        (0..24).any(|r| core.get_line_text(r).contains('m'))
    }
    // A final byte is displayed in ground and consumed inside a CSI.
    if shows_m(stream) {
        return None;
    }
    // `!` cancels the entry state and is an intermediate in the parameter state.
    Some(if shows_m(&[stream, b"!"].concat()) {
        CsiPhase::Entry
    } else {
        CsiPhase::Param
    })
}

const POST_STRIP_LAUNCH: &[u8] = b"\x1b]777;emterm;markdown;begin;id=x\x07";
const POST_STRIP_KITTY: &[u8] = b"\x1b_Gi=1,a=d;AAAA\x1b\\";

/// `(csi_in, designator flag, input, written bytes, reported state, whether
/// the written stream, started from the state given, ends in a CSI or in
/// ground, so that term_core can confirm the reported state)`.
type StateCase = (
    Option<CsiPhase>,
    bool,
    &'static [u8],
    &'static [u8],
    Option<CsiPhase>,
    bool,
);

/// The bytes that put term_core in a given CSI sub-state.
fn bytes_entering(phase: Option<CsiPhase>) -> &'static [u8] {
    match phase {
        None => b"",
        Some(CsiPhase::Entry) => b"\x1b[",
        Some(CsiPhase::Param) => b"\x1b[6",
    }
}

/// The reported state is the CSI sub-state of the stream formed by the state
/// given followed by the bytes written: bytes the pass removes never advance
/// it; copied bytes, designator bytes and the C0 bytes re-emitted from a
/// removed CSI query do.
#[test]
fn post_strip_state_form_reports_the_csi_state_of_the_written_bytes() {
    use CsiPhase::{Entry, Param};
    let cases: &[StateCase] = &[
        // Nothing written, nothing changes.
        (None, false, b"", b"", None, true),
        (Some(Param), false, b"", b"", Some(Param), true),
        (Some(Entry), false, b"", b"", Some(Entry), true),
        // Copied bytes advance the state.
        (None, false, b"abc", b"abc", None, true),
        (None, false, b"\x1b[", b"\x1b[", Some(Entry), true),
        (None, false, b"\x1b[6", b"\x1b[6", Some(Param), true),
        (None, false, b"\x1b[?25", b"\x1b[?25", Some(Param), true),
        (None, false, b"\x1b[6 ", b"\x1b[6 ", Some(Param), true),
        (None, false, b"\x1b[\r", b"\x1b[\r", Some(Entry), true),
        (None, false, b"\x1b[6m", b"\x1b[6m", None, true),
        (None, false, b"\x1b[6\x7f", b"\x1b[6\x7f", None, true),
        (None, false, b"\x1b[!", b"\x1b[!", None, true),
        (Some(Entry), false, b"6", b"6", Some(Param), true),
        (Some(Param), false, b"6", b"6", Some(Param), true),
        (Some(Param), false, b"m", b"m", None, true),
        (Some(Entry), false, b"\r", b"\r", Some(Entry), true),
        // An ESC that is written aborts the open CSI.
        (Some(Param), false, b"\x1bX", b"\x1bX", None, true),
        (Some(Param), false, b"\x1b[", b"\x1b[", Some(Entry), true),
        (Some(Param), false, b"\x1b[7", b"\x1b[7", Some(Param), true),
        (
            Some(Param),
            false,
            b"\x1b]0;t\x07",
            b"\x1b]0;t\x07",
            None,
            true,
        ),
        (
            None,
            false,
            b"\x1b[6\x1b]0;t\x07",
            b"\x1b[6\x1b]0;t\x07",
            None,
            true,
        ),
        // An ESC that is not written (a removed construct) leaves it open.
        (
            None,
            false,
            b"\x1b[6\x1b]777;emterm;markdown;begin;id=x\x07",
            b"\x1b[6",
            Some(Param),
            true,
        ),
        (
            None,
            false,
            b"\x1b[6\x1b_Gi=1,a=d;AAAA\x1b\\",
            b"\x1b[6",
            Some(Param),
            true,
        ),
        (None, false, b"\x1b[6\x1b[6n", b"\x1b[6", Some(Param), true),
        (None, false, b"\x1b[\x1b[6n", b"\x1b[", Some(Entry), true),
        (
            Some(Param),
            false,
            POST_STRIP_LAUNCH,
            b"",
            Some(Param),
            true,
        ),
        (Some(Entry), false, POST_STRIP_KITTY, b"", Some(Entry), true),
        (Some(Param), false, b"\x1b[6n", b"", Some(Param), true),
        (Some(Param), false, b"\x1b[c", b"", Some(Param), true),
        // The C0 bytes re-emitted from a removed query execute inside an
        // open CSI and leave it open (EC-3).
        (
            None,
            false,
            b"\x1b[6\x1b[6\rn",
            b"\x1b[6\r",
            Some(Param),
            true,
        ),
        (
            None,
            false,
            b"\x1b[6\x1b[\x086n",
            b"\x1b[6\x08",
            Some(Param),
            true,
        ),
        (Some(Entry), false, b"\x1b[6\rn", b"\r", Some(Entry), true),
        // The designator flag: byte 0 is copied whatever it is, and the
        // stream continues from ground.
        (None, true, b"B", b"B", None, true),
        (None, true, b"B\x1b[6", b"B\x1b[6", Some(Param), true),
        (None, true, b"\x1b[6n", b"\x1b[6n", None, true),
        (None, true, b"B\x1b[6\x1b[6n", b"B\x1b[6", Some(Param), true),
        // A trailing escape or designation start is not a CSI.
        (None, false, b"\x1b[6\x1b(", b"\x1b[6\x1b(", None, false),
        (None, false, b"\x1b[6\x1b", b"\x1b[6\x1b", None, false),
        (None, false, b"\x1b(\x1b", b"\x1b(\x1b", None, true),
    ];
    for (csi_in, designator, input, expected_out, expected_state, confirm) in cases {
        let label = format!(
            "state {csi_in:?}, designator {designator}, input {:?}",
            String::from_utf8_lossy(input)
        );
        let (out, state) =
            strip_pty_output_for_scrollback_write_with_csi_state(input, *designator, *csi_in);
        assert_eq!(out, *expected_out, "{label}: written bytes");
        assert_eq!(state, *expected_state, "{label}: reported state");
        if *confirm && !*designator {
            // The reported state is the one term_core is in after the bytes
            // that enter the given state followed by the written bytes.
            let stream = [bytes_entering(*csi_in), &out[..]].concat();
            assert_eq!(
                client_csi_phase(&stream),
                state,
                "{label}: term_core on the written stream"
            );
        }
    }
}

/// `csi_step` is the per-byte CSI transition both the write filter's scan and
/// the state-reporting strip follow: every byte in each sub-state is classified
/// as term_core does - the byte keeps the CSI open in the entry or the
/// parameter state, completes or cancels it, or (ESC) aborts it.
#[test]
fn post_strip_csi_step_classifies_every_byte_like_term_core() {
    for phase in [CsiPhase::Entry, CsiPhase::Param] {
        let head = bytes_entering(Some(phase));
        for byte in 0u8..=255 {
            let step = csi_step(phase, byte);
            if byte == 0x1b {
                assert_eq!(step, CsiStep::Abort, "{phase:?}: ESC aborts");
                continue;
            }
            let after = client_csi_phase(&[head, &[byte][..]].concat());
            let expected = match after {
                Some(next) => CsiStep::Open(next),
                None => CsiStep::End,
            };
            assert_eq!(step, expected, "{phase:?} then byte {byte:#04x}");
        }
    }
}

/// Inputs for the output-identity check: strip targets of every kind, kept
/// look-alikes, designations and text.
fn post_strip_identity_corpus() -> Vec<Vec<u8>> {
    let launch = osc(b"777;emterm;markdown;begin", b"\x07");
    let mut corpus: Vec<Vec<u8>> = vec![
        b"plain".to_vec(),
        b"a\x1b[6nb".to_vec(),
        b"\x1b(B\x1b)0text".to_vec(),
        b"\x1b_Gi=1;P\x1b\\tail".to_vec(),
        b"\x1bPq#0\x1b\\tail".to_vec(),
        b"\x1bP$qm\x1b\\tail".to_vec(),
        b"\x1b]0;title\x07".to_vec(),
        b"\x1b".to_vec(),
        b"\x1b[6\x1b".to_vec(),
        Vec::new(),
        b"\x1b[6\x1b[6n\x1b[5n\x1b[c\x1b[?c\x1b[>0c\x1b[14t\x1b[?25$p".to_vec(),
        b"\x1b[=c\x1b[0n\x1b[8;24;80t\x1b[261n\x1b[6\rn\x1b[\x086n".to_vec(),
        b"\x1b[6\x1b]777;emterm;markdown;x\x07\x1b_Gi=1;P\x1b\\\x1b[6n".to_vec(),
        b"\x1b]777;emterm;fold;start;1\x07\x1b]9999;emterm-mux;state;1\x07".to_vec(),
        b"\x1b[6\x1b]777;emterm;markdown;unterminated".to_vec(),
    ];
    corpus.push([b"x".as_slice(), &launch, b"y"].concat());
    corpus.extend(DESIGNATOR_ESC_KEPT_RUNS.iter().map(|r| r.to_vec()));
    for body in NON_CANONICAL_STRIPPED_BODIES.iter().chain(KEPT_BODIES) {
        for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
            let mut input = b"before\x1b[6".to_vec();
            input.extend_from_slice(&osc(body, terminator));
            input.extend_from_slice(b"after");
            corpus.push(input);
        }
    }
    corpus
}

/// R9 (AC-7, NFR1, FR6): the state-reporting form returns output byte-identical
/// to the existing write-path form over the scrollback_filter corpora, with the
/// designator flag on and off and from every CSI state given. The existing
/// forms keep their signatures and outputs.
#[test]
fn post_strip_state_form_output_equals_the_write_path_strip() {
    let configs: [(bool, Option<CsiPhase>); 4] = [
        (false, None),
        (false, Some(CsiPhase::Entry)),
        (false, Some(CsiPhase::Param)),
        (true, None),
    ];
    let mut check = |input: &[u8]| {
        for (designator, csi_in) in configs {
            let (out, _state) =
                strip_pty_output_for_scrollback_write_with_csi_state(input, designator, csi_in);
            assert_eq!(
                out,
                strip_pty_output_for_scrollback_write_with_designator(input, designator),
                "{input:?}: designator {designator}, state {csi_in:?}"
            );
            if !designator && csi_in.is_none() {
                assert_eq!(
                    out,
                    strip_pty_output_for_scrollback_write(input),
                    "{input:?}"
                );
                assert_eq!(out, strip_replayable_rich_content(input), "{input:?}");
            }
        }
    };
    for input in post_strip_identity_corpus() {
        check(&input);
    }
    // Every string of up to five bytes over an alphabet that forms the short
    // strip targets (a CSI query, a Kitty APC, a SIXEL DCS) and designations.
    let alphabet: [u8; 10] = [0x1b, b'[', b'(', b'_', b'P', b'G', b'q', b'6', b'n', b'\\'];
    let mut input: Vec<u8> = Vec::new();
    fn walk(alphabet: &[u8], input: &mut Vec<u8>, depth: usize, check: &mut impl FnMut(&[u8])) {
        check(input);
        if depth == 0 {
            return;
        }
        for &byte in alphabet {
            input.push(byte);
            walk(alphabet, input, depth - 1, check);
            input.pop();
        }
    }
    walk(&alphabet, &mut input, 5, &mut check);
}

/// NFR2: the state is advanced inside the strip's single pass, so a long input
/// stays linear: no second pass and no per-construct rescan.
#[test]
fn post_strip_state_form_is_one_linear_pass() {
    let unit = [
        b"\x1b[6".as_slice(),
        POST_STRIP_LAUNCH,
        b"\x1b[7",
        POST_STRIP_KITTY,
    ]
    .concat();
    let input: Vec<u8> = std::iter::repeat_n(unit, 100_000).flatten().collect();
    let start = std::time::Instant::now();
    let (out, state) = strip_pty_output_for_scrollback_write_with_csi_state(&input, false, None);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
    assert_eq!(out, b"\x1b[6\x1b[7".repeat(100_000));
    assert_eq!(state, Some(CsiPhase::Param));
}
