//! Builder-level tests for the wrap-aware snapshot layout
//! (mux-snapshot-ring-wrap-restore task0001, AC-3/4/5/6/7).
//!
//! These exercise [`super::build_snapshot_bytes_for_ring`] /
//! [`super::build_resume_snapshot_bytes_for_ring`] end to end: byte identity
//! with the plain (pre-fix) builders in the cases that must stay untouched
//! (AC-4), and the dump-block restore behavior (region / origin mode /
//! saved-cursor / continued-output / scrollback-history) for the wrapped
//! main-buffer case (AC-3/5/6). `dump_block`'s own unit tests cover the
//! composer's internals in isolation; these drive it through the public
//! builders with a real `vt100::Parser` standing in for the daemon shadow
//! parser (a leaf-safe substitute: this file never imports `mux::session`).

use super::*;
use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
use term_core::terminal_core::{MODE_ORIGIN, ReplaySegment, TerminalCore};

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

/// Build a ring + shadow-parser pair fed the same bytes, small enough that
/// the ring has wrapped by the time `stream` finishes.
fn wrapped_ring_and_shadow_dump(
    cols: u16,
    rows: u16,
    capacity: usize,
    stream: &[u8],
) -> (Vec<u8>, Vec<(usize, u16, u16)>, bool, Vec<u8>) {
    let mut ring = ScrollbackRingBuffer::new(capacity);
    ring.attribute_write(cols, rows, stream);
    let (raw, segments, wrapped) = ring.read_segments_with_wrap_state();

    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(stream);
    let shadow_dump = parser.screen().contents_formatted();

    (raw, segments, wrapped, shadow_dump)
}

fn row_text(parser: &vt100::Parser, cols: u16, row: u16) -> String {
    parser
        .screen()
        .rows(0, cols)
        .nth(row as usize)
        .unwrap_or_default()
}

// ── AC-4: byte identity when not wrapped, and for alt-screen regardless
// of the wrap flag ─────────────────────────────────────────────────────

#[test]
fn build_snapshot_bytes_for_ring_non_wrapped_matches_the_plain_builder_exactly() {
    let scrollback = b"below-capacity-history";
    let segments = [(0usize, 80u16, 24u16)];
    let screen = b"SCREEN-DUMP";
    for alt in [false, true] {
        let (plain, plain_segs) =
            build_snapshot_bytes(scrollback, &segments, screen, alt, (80, 24));
        let (ring_aware, ring_segs) =
            build_snapshot_bytes_for_ring(scrollback, &segments, screen, alt, false, (80, 24));
        assert_eq!(
            plain, ring_aware,
            "alt={alt}: non-wrapped payload must match exactly"
        );
        assert_eq!(
            plain_segs, ring_segs,
            "alt={alt}: non-wrapped segments must match exactly"
        );
    }
}

#[test]
fn build_snapshot_bytes_for_ring_alt_screen_matches_the_plain_builder_even_when_wrapped() {
    let scrollback = b"alt-history";
    let segments = [(0usize, 80u16, 24u16)];
    let screen = b"ALT-SCREEN-DUMP";
    let (plain, plain_segs) = build_snapshot_bytes(scrollback, &segments, screen, true, (80, 24));
    let (ring_aware, ring_segs) =
        build_snapshot_bytes_for_ring(scrollback, &segments, screen, true, true, (80, 24));
    assert_eq!(
        plain, ring_aware,
        "alt-screen output must stay byte-identical regardless of ring_wrapped"
    );
    assert_eq!(plain_segs, ring_segs);
}

#[test]
fn build_resume_snapshot_bytes_for_ring_non_wrapped_matches_the_plain_builder_exactly() {
    let scrollback = b"resume-history";
    let segments = [(0usize, 80u16, 24u16)];
    let screen = b"SCREEN";
    for alt in [false, true] {
        let (plain, plain_segs) =
            build_resume_snapshot_bytes(scrollback, &segments, screen, alt, (80, 24));
        let (ring_aware, ring_segs) = build_resume_snapshot_bytes_for_ring(
            scrollback,
            &segments,
            screen,
            alt,
            false,
            (80, 24),
        );
        assert_eq!(plain, ring_aware, "alt={alt}");
        assert_eq!(plain_segs, ring_segs, "alt={alt}");
    }
}

#[test]
fn build_resume_snapshot_bytes_for_ring_alt_screen_matches_the_plain_builder_even_when_wrapped() {
    let scrollback = b"resume-alt-history";
    let segments = [(0usize, 80u16, 24u16)];
    let screen = b"ALT-SCREEN";
    let (plain, plain_segs) =
        build_resume_snapshot_bytes(scrollback, &segments, screen, true, (80, 24));
    let (ring_aware, ring_segs) =
        build_resume_snapshot_bytes_for_ring(scrollback, &segments, screen, true, true, (80, 24));
    assert_eq!(plain, ring_aware);
    assert_eq!(plain_segs, ring_segs);
}

/// A genuinely at/below-capacity ring (not a hand-fed flag) must also
/// resolve to `ring_wrapped == false` and produce the identical payload —
/// closes the loop between `read_segments_with_wrap_state` and the
/// wrap-aware builder.
#[test]
fn build_snapshot_bytes_for_ring_a_ring_at_capacity_reports_not_wrapped_and_matches_plain_output() {
    let cols = 80u16;
    let rows = 24u16;
    let stream = b"short line\r\n";
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 4096, stream);
    assert!(!wrapped, "small stream must not exceed a 4096B capacity");

    let (plain, plain_segs) =
        build_snapshot_bytes(&raw, &segments, &shadow_dump, false, (cols, rows));
    let (ring_aware, ring_segs) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, (cols, rows));
    assert_eq!(plain, ring_aware);
    assert_eq!(plain_segs, ring_segs);
}

// ── AC-3: decoded segments end with the dump block, and replay reproduces
// the shadow parser's own visible screen row for row ────────────────────

#[test]
fn build_snapshot_bytes_for_ring_wrapped_main_buffer_restores_the_shadow_parsers_visible_screen() {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    stream.extend_from_slice(b"\x1b[1;1HHEADER-ROW-TEXT\r\n");
    for i in 0..80u32 {
        stream.extend_from_slice(format!("\x1b[2;1Hframe {i:>4}   \r\n").as_bytes());
    }
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped, "512B capacity must have wrapped for this stream");
    assert!(
        !segments.is_empty(),
        "attribute_write must record at least one segment"
    );

    let (payload, out_segments) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, (cols, rows));

    assert_eq!(
        out_segments.len(),
        segments.len() + 1,
        "the dump block must add exactly one trailing segment"
    );
    let last = *out_segments.last().unwrap();
    assert_eq!(
        (last.1, last.2),
        (cols, rows),
        "trailing segment carries current_dims"
    );
    assert!(
        last.0 < payload.len(),
        "dump block start must be before the end of the payload"
    );

    let mut core = TerminalCore::new(cols, rows, 10_000);
    core.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&stream);
    for r in 0..rows {
        let got = core.get_line_text(r);
        let want = row_text(&parser, cols, r);
        assert_eq!(got.trim_end(), want.trim_end(), "row {r} mismatch");
    }
    assert!(
        core.get_line_text(0).contains("HEADER-ROW-TEXT"),
        "row 0 must contain the header text restored from the shadow parser"
    );
}

/// The shadow parser's dims (== `current_dims`) can legitimately differ
/// from the ring's last recorded segment dims (a stale segment left behind
/// by an `attribute_write` correction, D7''-style). The trailing dump
/// segment — and the dump block's own drawing — must use `current_dims`,
/// not whatever the ring's segments say.
#[test]
fn build_snapshot_bytes_for_ring_uses_current_dims_even_when_it_differs_from_the_last_ring_segment()
{
    let ring_cols = 80u16;
    let ring_rows = 24u16;
    let stream = {
        let mut s = Vec::new();
        for i in 0..40u32 {
            s.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        s
    };
    let (raw, segments, wrapped, _unused_shadow) =
        wrapped_ring_and_shadow_dump(ring_cols, ring_rows, 128, &stream);
    assert!(wrapped);
    assert_eq!(
        segments.last().map(|&(_, c, r)| (c, r)),
        Some((ring_cols, ring_rows)),
        "precondition: the ring's own recorded dims are the smaller (80,24)"
    );

    // The shadow parser (and therefore current_dims) has since moved on to
    // a LARGER size that the ring never recorded a segment for.
    let current_dims = (100u16, 30u16);
    let mut parser = vt100::Parser::new(current_dims.1, current_dims.0, 0);
    parser.process(&stream);
    let shadow_dump = parser.screen().contents_formatted();

    let (payload, out_segments) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, current_dims);
    let last = *out_segments.last().unwrap();
    assert_eq!(
        (last.1, last.2),
        current_dims,
        "trailing segment must carry current_dims, not the ring's stale (80,24)"
    );

    let mut core = TerminalCore::new(current_dims.0, current_dims.1, 10_000);
    core.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));
    for r in 0..current_dims.1 {
        let got = core.get_line_text(r);
        let want = row_text(&parser, current_dims.0, r);
        assert_eq!(
            got.trim_end(),
            want.trim_end(),
            "row {r} mismatch at current_dims"
        );
    }
}

// ── AC-5(b): narrowed scroll region + origin mode restored after the
// dump block, matching an oracle replay of the delegated payload alone ──

#[test]
fn build_snapshot_bytes_for_ring_restores_probe_state_after_narrowed_region_and_origin_mode() {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    stream.extend_from_slice(b"\x1b[5;20r"); // narrow region, rows 5..20 (1-indexed)
    stream.extend_from_slice(b"\x1b[?6h"); // origin mode on
    stream.extend_from_slice(b"\x1b[3;10H"); // region-relative cursor position
    stream.extend_from_slice(b"\x1b[1;35mtail-styled");

    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped, "512B capacity must have wrapped for this stream");

    let (payload, out_segments) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, (cols, rows));

    // Oracle: independently replay the DELEGATED (pre-dump) payload alone
    // to compute the expected probe state.
    let (delegated_payload, delegated_segments) =
        build_snapshot_bytes(&raw, &segments, &shadow_dump, false, (cols, rows));
    let mut oracle = TerminalCore::new(cols, rows, 0);
    oracle.reset_and_replay_segments(&delegated_payload, &to_replay_segments(&delegated_segments));

    let mut core = TerminalCore::new(cols, rows, 10_000);
    core.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    assert_eq!(core.get_scroll_region_top(), oracle.get_scroll_region_top());
    assert_eq!(
        core.get_scroll_region_bottom(),
        oracle.get_scroll_region_bottom()
    );
    assert_eq!(core.get_mode(MODE_ORIGIN), oracle.get_mode(MODE_ORIGIN));
    assert_eq!(core.get_cursor_row(), oracle.get_cursor_row());
    assert_eq!(core.get_cursor_col(), oracle.get_cursor_col());
    assert_eq!(core.get_cursor_fg(), oracle.get_cursor_fg());
    assert_eq!(core.get_cursor_bg(), oracle.get_cursor_bg());
    assert_eq!(core.get_cursor_flags(), oracle.get_cursor_flags());

    // Each dumped row lands at the shadow parser's own row index.
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&stream);
    for r in 0..rows {
        let got = core.get_line_text(r);
        let want = row_text(&parser, cols, r);
        assert_eq!(got.trim_end(), want.trim_end(), "row {r} mismatch");
    }
}

// ── AC-5(c): a pending saved cursor (app-level DECSC, no DECRC yet) is
// not consumed or replaced by the dump block ─────────────────────────────

#[test]
fn build_snapshot_bytes_for_ring_preserves_a_pending_application_saved_cursor() {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    stream.extend_from_slice(b"\x1b7"); // DECSC pending, no DECRC yet

    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 256, &stream);
    assert!(wrapped, "256B capacity must have wrapped for this stream");

    let (payload, out_segments) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, (cols, rows));

    let mut reference = TerminalCore::new(cols, rows, 0);
    reference.process_pty_data_fully(&stream);
    reference.process_pty_data_fully(b"\x1b8AFTER-DECRC");

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));
    client.process_pty_data_fully(b"\x1b8AFTER-DECRC");

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch after DECRC: the dump block must not have consumed or \
             replaced the application's pending saved cursor"
        );
    }
}

// ── AC-5(d): no dump block ever contains DECSC/DECRC, including the case
// where vt100 itself parks the cursor past the end of a row ─────────────

/// A real vt100 dump that DOES emit the SaveCursor/RestoreCursor dance:
/// filling the last row completely (leaving the cursor in the pending-wrap
/// position) and then erasing that same line (`CSI 2 K`) clears the last
/// cell's contents while the cursor position stays pending-wrap and no
/// earlier row has a last-column cell with contents either — exactly the
/// fallback branch in vt100's `write_cursor_position_formatted` that emits
/// `ESC 7 ... ESC 8` to park the cursor (confirmed empirically against the
/// vt100 0.16.2 dependency; see dump_block.rs's doc comment for the
/// general rationale).
#[test]
fn build_snapshot_bytes_for_ring_never_emits_decsc_or_decrc_even_when_the_shadow_parser_parks_the_cursor_past_a_row()
 {
    let cols = 80u16;
    let rows = 24u16;
    let mut fill_stream = Vec::new();
    for i in 0..23u32 {
        fill_stream.extend_from_slice(format!("row{i}\r\n").as_bytes());
    }
    fill_stream.extend_from_slice(&vec![b'x'; cols as usize]); // pending wrap on the last row
    fill_stream.extend_from_slice(b"\x1b[2K"); // erase whole line, cursor stays put

    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&fill_stream);
    let shadow_dump = parser.screen().contents_formatted();
    assert!(
        shadow_dump.windows(2).any(|w| w == b"\x1b7"),
        "precondition: this fixture must make vt100 itself emit the SaveCursor dance \
         (dump: {:?})",
        String::from_utf8_lossy(&shadow_dump)
    );

    let ring_stream = fill_stream; // same bytes drive the ring so it also wraps.
    let mut ring = ScrollbackRingBuffer::new(64);
    ring.attribute_write(cols, rows, &ring_stream);
    let (raw, segments, wrapped) = ring.read_segments_with_wrap_state();
    assert!(wrapped, "64B capacity must have wrapped for this stream");

    let (payload, _out_segments) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, (cols, rows));

    assert!(
        !payload.windows(2).any(|w| w == b"\x1b7"),
        "composed snapshot must never contain DECSC"
    );
    assert!(
        !payload.windows(2).any(|w| w == b"\x1b8"),
        "composed snapshot must never contain DECRC"
    );
}

// ── AC-6: continued output after a post-wrap replay matches a reference
// terminal fed the whole stream; scrollback history matches a replay of
// the payload truncated at the dump block start ─────────────────────────

#[test]
fn build_snapshot_bytes_for_ring_continued_output_matches_a_reference_fed_the_whole_stream() {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    stream.extend_from_slice(b"\x1b[1;1HHEADER\r\n");
    for i in 0..80u32 {
        stream.extend_from_slice(format!("\x1b[2;1Hframe {i:>4}\r\n").as_bytes());
    }
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped);

    let (payload, out_segments) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, (cols, rows));

    let continued = b"\r\nCONTINUED-OUTPUT-LINE";

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&stream);
    reference.process_pty_data_fully(continued);

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));
    client.process_pty_data_fully(continued);

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch for output continued after the snapshot"
        );
    }
}

#[test]
fn build_snapshot_bytes_for_ring_scrollback_history_matches_a_replay_truncated_at_the_dump_block_start()
 {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("scroll-line {i}\r\n").as_bytes());
    }
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped);

    let (payload, out_segments) =
        build_snapshot_bytes_for_ring(&raw, &segments, &shadow_dump, false, wrapped, (cols, rows));
    let dump_start = out_segments.last().expect("dump segment must exist").0;

    let mut full = TerminalCore::new(cols, rows, 10_000);
    full.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    let truncated_payload = &payload[..dump_start];
    let truncated_segments: Vec<(usize, u16, u16)> = out_segments
        .iter()
        .copied()
        .filter(|&(offset, _, _)| offset < dump_start)
        .collect();
    let mut truncated = TerminalCore::new(cols, rows, 10_000);
    truncated
        .reset_and_replay_segments(truncated_payload, &to_replay_segments(&truncated_segments));

    assert_eq!(
        full.get_scrollback_length(),
        truncated.get_scrollback_length(),
        "the dump block must draw only the visible screen, never push scrollback history"
    );
    for i in 0..full.get_scrollback_length() {
        assert_eq!(
            full.get_scrollback_text(i),
            truncated.get_scrollback_text(i),
            "scrollback line {i} must be unaffected by the dump block"
        );
    }
}

// ── AC-7(e): a failed probe degrades to the delegated (non-wrapped)
// output at the builder level ────────────────────────────────────────────

#[test]
fn build_snapshot_bytes_for_ring_degrades_to_the_delegated_output_when_the_probe_fails() {
    let scrollback = b"history";
    let segments = [(0usize, 80u16, 24u16)];
    let screen = b"SHADOW-DUMP";
    // A degenerate current_dims (either axis zero) makes the probe fail.
    let degenerate_dims = (0u16, 24u16);

    let (delegated, delegated_segments) =
        build_snapshot_bytes(scrollback, &segments, screen, false, degenerate_dims);
    let (ring_aware, ring_segments) =
        build_snapshot_bytes_for_ring(scrollback, &segments, screen, false, true, degenerate_dims);
    assert_eq!(
        ring_aware, delegated,
        "probe failure must degrade to the delegated payload"
    );
    assert_eq!(ring_segments, delegated_segments);
}

#[test]
fn build_snapshot_bytes_for_ring_degrades_when_the_shadow_dump_is_empty() {
    let scrollback = b"history";
    let segments = [(0usize, 80u16, 24u16)];
    let (delegated, delegated_segments) =
        build_snapshot_bytes(scrollback, &segments, b"", false, (80, 24));
    let (ring_aware, ring_segments) =
        build_snapshot_bytes_for_ring(scrollback, &segments, b"", false, true, (80, 24));
    assert_eq!(
        ring_aware, delegated,
        "an empty shadow dump has nothing to restore"
    );
    assert_eq!(ring_segments, delegated_segments);
}
