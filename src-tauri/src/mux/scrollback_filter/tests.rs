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
