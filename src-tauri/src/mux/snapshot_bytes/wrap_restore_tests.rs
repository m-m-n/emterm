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
        let (ring_aware, ring_segs) = build_snapshot_bytes_for_ring(
            scrollback,
            &segments,
            screen,
            alt,
            false,
            (80, 24),
            10_000,
        );
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
        build_snapshot_bytes_for_ring(scrollback, &segments, screen, true, true, (80, 24), 10_000);
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
            10_000,
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
    let (ring_aware, ring_segs) = build_resume_snapshot_bytes_for_ring(
        scrollback,
        &segments,
        screen,
        true,
        true,
        (80, 24),
        10_000,
    );
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
    let (ring_aware, ring_segs) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );
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

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

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

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        current_dims,
        10_000,
    );
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

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

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

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

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

    let (payload, _out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

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

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

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

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );
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
    let (ring_aware, ring_segments) = build_snapshot_bytes_for_ring(
        scrollback,
        &segments,
        screen,
        false,
        true,
        degenerate_dims,
        10_000,
    );
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
        build_snapshot_bytes_for_ring(scrollback, &segments, b"", false, true, (80, 24), 10_000);
    assert_eq!(
        ring_aware, delegated,
        "an empty shadow dump has nothing to restore"
    );
    assert_eq!(ring_segments, delegated_segments);
}

// ── AC-3/AC-4 (task0002, D7): a pending wrap survives the wrapped-ring
// restore, and one more printable character wraps identically to a
// whole-stream reference ─────────────────────────────────────────────────

/// Asserts every visible row, plus the cursor row/col, are equal between
/// two cores at the same dims — the shared post-continuation check every
/// AC-3/AC-4 case below runs after feeding one more printable character.
fn assert_rows_and_cursor_match(client: &TerminalCore, reference: &TerminalCore, rows: u16) {
    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch after continued output"
        );
    }
    assert_eq!(client.get_cursor_row(), reference.get_cursor_row());
    assert_eq!(client.get_cursor_col(), reference.get_cursor_col());
}

/// AC-3: a wrapped main-buffer stream whose last output exactly fills a
/// row leaves the cursor with a pending wrap. After a snapshot replay, the
/// client's pending-wrap flag and cursor position equal the probe's (an
/// oracle replay of the delegated payload alone), and its visible rows
/// equal the shadow parser's. Appending one printable character to both
/// the client and a whole-stream reference gives equal visible rows and
/// cursor positions: the character goes to the next row's first column,
/// and the last column keeps its character.
#[test]
fn build_snapshot_bytes_for_ring_restores_a_pending_wrap_and_matches_a_reference_after_one_more_char()
 {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    // Fill the last row exactly (no trailing newline): a pending wrap.
    stream.extend_from_slice(&vec![b'X'; cols as usize]);

    // Capacity chosen to wrap (below the ~610B full stream) while still
    // retaining more than a full screen's worth of "line N\r\n" tail (a ring
    // any smaller loses enough scroll history that replaying the retained
    // tail alone from blank no longer lands the cursor on the bottom row via
    // genuine scrolling, which would make the probe's own cursor-row
    // computation diverge from a whole-stream reference for reasons
    // unrelated to what this test exercises).
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped, "512B capacity must have wrapped for this stream");

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

    // Oracle: independently replay the DELEGATED (pre-dump) payload alone
    // to compute the expected probe state (mirrors the AC-5(b)-style
    // narrowed-region test above, at the builder level).
    let (delegated_payload, delegated_segments) =
        build_snapshot_bytes(&raw, &segments, &shadow_dump, false, (cols, rows));
    let mut oracle = TerminalCore::new(cols, rows, 0);
    oracle.reset_and_replay_segments(&delegated_payload, &to_replay_segments(&delegated_segments));
    assert!(
        oracle.get_wrap_pending(),
        "test prerequisite: filling the last row exactly must leave a pending wrap"
    );

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    assert_eq!(client.get_wrap_pending(), oracle.get_wrap_pending());
    assert_eq!(client.get_cursor_row(), oracle.get_cursor_row());
    assert_eq!(client.get_cursor_col(), oracle.get_cursor_col());

    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&stream);
    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            row_text(&parser, cols, r).trim_end(),
            "row {r} mismatch"
        );
    }

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&stream);
    reference.process_pty_data_fully(b"Y");

    client.process_pty_data_fully(b"Y");

    assert_rows_and_cursor_match(&client, &reference, rows);
}

/// AC-4(a): the pending-wrap row lies inside a narrowed scroll region with
/// origin mode on.
#[test]
fn build_snapshot_bytes_for_ring_restores_a_pending_wrap_inside_a_narrowed_region_with_origin_mode()
{
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    stream.extend_from_slice(b"\x1b[5;20r"); // narrow region, rows 5..20 (1-indexed)
    stream.extend_from_slice(b"\x1b[?6h"); // origin mode on
    stream.extend_from_slice(b"\x1b[3;1H"); // region-relative row 3, col 1
    // Fill the row exactly (region-relative row 3, absolute row 6): a
    // pending wrap, same as AC-3 but inside the narrowed/origin-mode case.
    stream.extend_from_slice(&vec![b'Z'; cols as usize]);

    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped, "512B capacity must have wrapped for this stream");

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

    let (delegated_payload, delegated_segments) =
        build_snapshot_bytes(&raw, &segments, &shadow_dump, false, (cols, rows));
    let mut oracle = TerminalCore::new(cols, rows, 0);
    oracle.reset_and_replay_segments(&delegated_payload, &to_replay_segments(&delegated_segments));
    assert!(
        oracle.get_wrap_pending(),
        "test prerequisite: filling the region row exactly must leave a pending wrap"
    );

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    assert_eq!(client.get_wrap_pending(), oracle.get_wrap_pending());
    assert_eq!(client.get_cursor_row(), oracle.get_cursor_row());
    assert_eq!(client.get_cursor_col(), oracle.get_cursor_col());
    assert_eq!(
        client.get_scroll_region_top(),
        oracle.get_scroll_region_top()
    );
    assert_eq!(
        client.get_scroll_region_bottom(),
        oracle.get_scroll_region_bottom()
    );
    assert_eq!(client.get_mode(MODE_ORIGIN), oracle.get_mode(MODE_ORIGIN));

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&stream);
    reference.process_pty_data_fully(b"Y");

    client.process_pty_data_fully(b"Y");

    assert_rows_and_cursor_match(&client, &reference, rows);
}

/// AC-4(b): the pending-wrap row ends with a double-width character in its
/// last two columns — the composer must re-establish the wrap by
/// re-printing the wide-pair's BASE, not a lone spacer.
#[test]
fn build_snapshot_bytes_for_ring_restores_a_pending_wrap_ending_in_a_double_width_character() {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    // Fill the row with (cols - 2) narrow chars, then one wide (2-column)
    // character occupying the last two columns: a pending wrap.
    stream.extend_from_slice(&vec![b'a'; (cols - 2) as usize]);
    stream.extend_from_slice("世".as_bytes());

    // See the capacity comment on the AC-3 test above: large enough to
    // retain more than a full screen's worth of scroll history so the
    // probe's cursor-row replay reaches the bottom row by genuine scrolling.
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped, "512B capacity must have wrapped for this stream");

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

    let (delegated_payload, delegated_segments) =
        build_snapshot_bytes(&raw, &segments, &shadow_dump, false, (cols, rows));
    let mut oracle = TerminalCore::new(cols, rows, 0);
    oracle.reset_and_replay_segments(&delegated_payload, &to_replay_segments(&delegated_segments));
    assert!(
        oracle.get_wrap_pending(),
        "test prerequisite: filling the row with a trailing wide char must leave a pending wrap"
    );

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    assert_eq!(client.get_wrap_pending(), oracle.get_wrap_pending());
    assert_eq!(client.get_cursor_row(), oracle.get_cursor_row());
    assert_eq!(client.get_cursor_col(), oracle.get_cursor_col());

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&stream);
    reference.process_pty_data_fully(b"Y");

    client.process_pty_data_fully(b"Y");

    assert_rows_and_cursor_match(&client, &reference, rows);
}

// ── mux-probe-scrollback-capacity task0002, AC-4/FR9: a client-reported
// probe capacity C, combined with a grow-resize, restores continued output
// to the row a capacity-C-only client would actually reach — through BOTH
// client replay entry points (the synchronous `reset_and_replay_segments`
// and the off-thread `build_from_snapshot` bypass path) ─────────────────

/// Rows-only grow (same width): 80x24 for the first half of the stream,
/// resized in lockstep on both the ring and the shadow parser to 80x40 for
/// the second half — mirrors `dump_block.rs`'s AC-5(a) shape, but through a
/// real [`ScrollbackRingBuffer`] + `vt100::Parser` pair so
/// `build_snapshot_bytes_for_ring` sees a genuinely wrapped ring.
fn fixture_rows_only_grow(
    ring_capacity: usize,
) -> (Vec<u8>, Vec<(usize, u16, u16)>, bool, Vec<u8>, (u16, u16)) {
    let initial = (80u16, 24u16);
    let grown = (80u16, 40u16);
    let mut pre = Vec::new();
    for i in 0..60u32 {
        pre.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    let mut post = Vec::new();
    for i in 60..90u32 {
        post.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }

    let mut ring = ScrollbackRingBuffer::new(ring_capacity);
    let mut parser = vt100::Parser::new(initial.1, initial.0, 0);
    ring.attribute_write(initial.0, initial.1, &pre);
    parser.process(&pre);
    ring.attribute_write(grown.0, grown.1, &post);
    parser.screen_mut().set_size(grown.1, grown.0);
    parser.process(&post);

    let (raw, segments, wrapped) = ring.read_segments_with_wrap_state();
    let shadow_dump = parser.screen().contents_formatted();
    (raw, segments, wrapped, shadow_dump, grown)
}

/// Rows AND columns grow together in the same resize (exercises the
/// full-reflow path, not the same-width path `fixture_rows_only_grow`
/// exercises) — mirrors `dump_block.rs`'s AC-5(b) shape.
fn fixture_rows_and_cols_grow(
    ring_capacity: usize,
) -> (Vec<u8>, Vec<(usize, u16, u16)>, bool, Vec<u8>, (u16, u16)) {
    let initial = (80u16, 24u16);
    let grown = (100u16, 40u16);
    let mut pre = Vec::new();
    for i in 0..60u32 {
        pre.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    let mut post = Vec::new();
    for i in 60..90u32 {
        post.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }

    let mut ring = ScrollbackRingBuffer::new(ring_capacity);
    let mut parser = vt100::Parser::new(initial.1, initial.0, 0);
    ring.attribute_write(initial.0, initial.1, &pre);
    parser.process(&pre);
    ring.attribute_write(grown.0, grown.1, &post);
    parser.screen_mut().set_size(grown.1, grown.0);
    parser.process(&post);

    let (raw, segments, wrapped) = ring.read_segments_with_wrap_state();
    let shadow_dump = parser.screen().contents_formatted();
    (raw, segments, wrapped, shadow_dump, grown)
}

/// `current_dims` is larger than the LAST (only) segment the ring ever
/// recorded — the grow reaches the client only through
/// `reset_and_replay_segments`'s own trailing "resize to the caller's
/// target" hop, never through an explicit segment (the ring was never told
/// about the resize — the shadow parser's own live size, tracking every
/// `MuxPane::resize`, is the only place `current_dims` comes from). Mirrors
/// `dump_block.rs`'s AC-5(c) shape, and the same divergence
/// `probe_replay_state_matches_the_oracle_when_current_dims_exceeds_the_last_segment`'s
/// doc comment records (row 23 at zero probe history vs row 39 against a
/// 10,000-line oracle).
fn fixture_current_dims_exceeds_the_last_segment(
    ring_capacity: usize,
) -> (Vec<u8>, Vec<(usize, u16, u16)>, bool, Vec<u8>, (u16, u16)) {
    let initial = (80u16, 24u16);
    let current_dims = (80u16, 40u16);
    let mut pre = Vec::new();
    for i in 0..60u32 {
        pre.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }

    let mut ring = ScrollbackRingBuffer::new(ring_capacity);
    ring.attribute_write(initial.0, initial.1, &pre);
    let (raw, segments, wrapped) = ring.read_segments_with_wrap_state();

    let mut parser = vt100::Parser::new(initial.1, initial.0, 0);
    parser.process(&pre);
    parser.screen_mut().set_size(current_dims.1, current_dims.0);
    let shadow_dump = parser.screen().contents_formatted();
    (raw, segments, wrapped, shadow_dump, current_dims)
}

/// Asserts `client`'s cursor row/col and the text of the row the cursor
/// sits on (the marker row — a single-line continuation with no CR/LF
/// leaves the cursor on the SAME row it just printed to) equal `oracle`'s
/// — the exact pair AC-4 names ("the cursor row/col, and the row holding
/// the marker"), not the whole screen: at a small `probe_capacity` the
/// oracle's OWN reconstruction of the delegated (pre-dump) payload is
/// itself an incomplete rebuild of history the ring has evicted (the very
/// gap the dump block exists to close for the REAL client — see
/// `build_snapshot_bytes_for_ring_wrapped_main_buffer_restores_the_shadow_parsers_visible_screen`),
/// so full-screen equality would fail for reasons unrelated to what AC-4
/// tests: whether the PROBED cursor state (and therefore where new output
/// lands) tracks the client's own capacity.
fn assert_ac4_cursor_and_marker_row_match(client: &TerminalCore, oracle: &TerminalCore) {
    assert_eq!(
        client.get_cursor_row(),
        oracle.get_cursor_row(),
        "cursor row must match the capacity-C oracle"
    );
    assert_eq!(
        client.get_cursor_col(),
        oracle.get_cursor_col(),
        "cursor col must match the capacity-C oracle"
    );
    let marker_row = oracle.get_cursor_row();
    assert_eq!(
        client.get_line_text(marker_row).trim_end(),
        oracle.get_line_text(marker_row).trim_end(),
        "the marker row's text must match the capacity-C oracle"
    );
}

/// Shared AC-4/FR9 assertion: build the wrapped-ring snapshot at
/// `probe_capacity`, replay it through BOTH client entry points (the
/// synchronous `reset_and_replay_segments` and the off-thread
/// `build_from_snapshot` bypass path), feed the same continuation to each,
/// and assert the cursor row/col plus the marker row's text match an
/// oracle that replays the DELEGATED (pre-dump, pre-fix) payload alone —
/// at the SAME `probe_capacity` — plus the same continuation.
/// `probe_capacity` here plays BOTH roles the task plan's Design section
/// describes as the same value: the probe's capacity (what the snapshot is
/// built with) and the client's own replay capacity (what a real client of
/// that reported capacity would replay with) — the whole point of AC-4 is
/// that these two agree.
fn assert_ac4_regression_case(
    raw: &[u8],
    segments: &[(usize, u16, u16)],
    shadow_dump: &[u8],
    current_dims: (u16, u16),
    probe_capacity: u32,
) {
    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        raw,
        segments,
        shadow_dump,
        false,
        true,
        current_dims,
        probe_capacity,
    );

    let continuation = b"\r\nMARKER-ROW-TEXT";

    // Oracle: capacity-`probe_capacity` replay of the DELEGATED (pre-dump,
    // pre-fix) payload alone, plus the same continuation.
    let (delegated_payload, delegated_segments) =
        build_snapshot_bytes(raw, segments, shadow_dump, false, current_dims);
    let mut oracle = TerminalCore::new(current_dims.0, current_dims.1, probe_capacity);
    oracle.reset_and_replay_segments(&delegated_payload, &to_replay_segments(&delegated_segments));
    oracle.process_pty_data_fully(continuation);

    // Path 1: the synchronous `reset_and_replay_segments` entry point (what
    // a real client's `apply_mux_message::Snapshot|SnapshotRestore` uses).
    let mut sync_client = TerminalCore::new(current_dims.0, current_dims.1, probe_capacity);
    sync_client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));
    sync_client.process_pty_data_fully(continuation);
    assert_ac4_cursor_and_marker_row_match(&sync_client, &oracle);

    // Path 2: the off-thread `build_from_snapshot` bypass path.
    let never = std::sync::atomic::AtomicBool::new(false);
    let replay = TerminalCore::build_from_snapshot(
        current_dims.0,
        current_dims.1,
        probe_capacity,
        &payload,
        &to_replay_segments(&out_segments),
        &never,
    )
    .expect("off-thread build must not be cancelled");
    let mut off_thread_client = replay.core;
    off_thread_client.process_pty_data_fully(continuation);
    assert_ac4_cursor_and_marker_row_match(&off_thread_client, &oracle);
}

#[test]
fn ac4_rows_only_grow_at_capacity_zero_matches_the_capacity_zero_oracle() {
    let (raw, segments, wrapped, shadow_dump, current_dims) = fixture_rows_only_grow(512);
    assert!(wrapped, "test prerequisite: the ring must have wrapped");
    assert_ac4_regression_case(&raw, &segments, &shadow_dump, current_dims, 0);
}

#[test]
fn ac4_rows_only_grow_at_a_small_nonzero_capacity_matches_the_same_capacity_oracle() {
    let (raw, segments, wrapped, shadow_dump, current_dims) = fixture_rows_only_grow(512);
    assert!(wrapped, "test prerequisite: the ring must have wrapped");
    assert_ac4_regression_case(&raw, &segments, &shadow_dump, current_dims, 5);
}

#[test]
fn ac4_rows_and_cols_grow_at_capacity_zero_matches_the_capacity_zero_oracle() {
    let (raw, segments, wrapped, shadow_dump, current_dims) = fixture_rows_and_cols_grow(512);
    assert!(wrapped, "test prerequisite: the ring must have wrapped");
    assert_ac4_regression_case(&raw, &segments, &shadow_dump, current_dims, 0);
}

#[test]
fn ac4_rows_and_cols_grow_at_a_small_nonzero_capacity_matches_the_same_capacity_oracle() {
    let (raw, segments, wrapped, shadow_dump, current_dims) = fixture_rows_and_cols_grow(512);
    assert!(wrapped, "test prerequisite: the ring must have wrapped");
    assert_ac4_regression_case(&raw, &segments, &shadow_dump, current_dims, 5);
}

/// The confirmed red case (Test Notes, per this fixture's own
/// `dump_block.rs`-anchored divergence — see
/// `probe_replay_state_matches_the_oracle_when_current_dims_exceeds_the_last_segment`'s
/// doc comment): at this level (through the builder, both replay paths,
/// and a continuation, rather than `probe_replay_state` alone) confirmed
/// by temporarily hard-coding `10_000` in place of `probe_capacity` in
/// `assert_ac4_regression_case`'s `build_snapshot_bytes_for_ring` call and
/// re-running both `current_dims_exceeds_the_last_segment` cases: BOTH
/// failed on a cursor-row mismatch against their own capacity-C oracle
/// (capacity 0: got row 39 forced-10,000, wanted row 24; capacity 5: got
/// row 39 forced-10,000, wanted row 29) — reverted to pass `probe_capacity`
/// through, both pass again. The other two shapes
/// (`fixture_rows_only_grow`, `fixture_rows_and_cols_grow`) did not diverge
/// for either capacity with this fixed-10,000 probe (same non-divergence
/// `dump_block.rs`'s AC-5(a)/(b) doc comments record at the
/// `probe_replay_state` level) and are kept as same-shape regression
/// guards, per the task plan's Test Notes fallback.
#[test]
fn ac4_current_dims_exceeds_the_last_segment_at_capacity_zero_matches_the_capacity_zero_oracle() {
    let (raw, segments, wrapped, shadow_dump, current_dims) =
        fixture_current_dims_exceeds_the_last_segment(512);
    assert!(wrapped, "test prerequisite: the ring must have wrapped");
    assert_ac4_regression_case(&raw, &segments, &shadow_dump, current_dims, 0);
}

#[test]
fn ac4_current_dims_exceeds_the_last_segment_at_a_small_nonzero_capacity_matches_the_same_capacity_oracle()
 {
    let (raw, segments, wrapped, shadow_dump, current_dims) =
        fixture_current_dims_exceeds_the_last_segment(512);
    assert!(wrapped, "test prerequisite: the ring must have wrapped");
    assert_ac4_regression_case(&raw, &segments, &shadow_dump, current_dims, 5);
}

// ── mux-probe-scrollback-capacity task0002, AC-2: a reported capacity
// above the 10,000 cap still carries the dump block and equals the output
// at 10,000 ────────────────────────────────────────────────────────────

#[test]
fn build_snapshot_bytes_for_ring_at_a_reported_capacity_above_the_cap_equals_the_output_at_the_cap()
{
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 512, &stream);
    assert!(wrapped, "512B capacity must have wrapped for this stream");

    let resolved_above_cap = resolve_probe_capacity(Some(20_000));
    assert_eq!(
        resolved_above_cap, 10_000,
        "test prerequisite: a reported capacity above the cap resolves to the cap"
    );

    let (at_resolved, segs_at_resolved) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        resolved_above_cap,
    );
    let (at_cap_directly, segs_at_cap_directly) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

    assert_eq!(
        at_resolved, at_cap_directly,
        "a reported capacity above the cap must produce byte-identical output to the cap itself"
    );
    assert_eq!(segs_at_resolved, segs_at_cap_directly);
}

// ── AC-4(c): a control case — the cursor sits at the last column with NO
/// pending wrap (placed there by absolute positioning, not by filling the
/// row via printing). The client's pending-wrap flag must stay clear, and
/// the next character OVERWRITES the last column instead of wrapping, as
/// on the reference.
#[test]
fn build_snapshot_bytes_for_ring_leaves_no_pending_wrap_when_the_cursor_was_placed_absolutely() {
    let cols = 80u16;
    let rows = 24u16;
    let mut stream = Vec::new();
    for i in 0..60u32 {
        stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    // Absolute positioning to the last column — no pending wrap, unlike
    // AC-3/AC-4(a)/(b) which reach the last column by printing into it.
    stream.extend_from_slice(format!("\x1b[1;{}H", cols).as_bytes());

    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 256, &stream);
    assert!(wrapped, "256B capacity must have wrapped for this stream");

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

    let (delegated_payload, delegated_segments) =
        build_snapshot_bytes(&raw, &segments, &shadow_dump, false, (cols, rows));
    let mut oracle = TerminalCore::new(cols, rows, 0);
    oracle.reset_and_replay_segments(&delegated_payload, &to_replay_segments(&delegated_segments));
    assert!(
        !oracle.get_wrap_pending(),
        "test prerequisite: absolute positioning must not leave a pending wrap"
    );

    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(&payload, &to_replay_segments(&out_segments));

    assert!(
        !client.get_wrap_pending(),
        "the client must not report a pending wrap the probe never reported"
    );
    assert_eq!(client.get_cursor_row(), oracle.get_cursor_row());
    assert_eq!(client.get_cursor_col(), oracle.get_cursor_col());

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&stream);
    reference.process_pty_data_fully(b"Y");

    client.process_pty_data_fully(b"Y");

    assert_rows_and_cursor_match(&client, &reference, rows);
}

// ── mux-probe-scrollback-capacity task0002, AC-3: byte-identical golden
// output at probe capacity 10,000 ────────────────────────────────────────

/// Golden-output pin (AC-3): at probe capacity 10,000 the wrap-aware
/// builder's output for a fixed wrapped fixture is exactly these literal
/// bytes/segments. This feature never changes `probe_replay_state`'s replay
/// logic, `compose_wrapped_dump_block`'s composition steps, or
/// `build_snapshot_bytes_with_layout` — it only threads a `probe_capacity`
/// parameter through, so the output at 10,000 (the value every pre-existing
/// call site passed as a hardcoded literal) is unchanged from before this
/// feature; this literal was captured directly from this fixture at HEAD of
/// this task's own changes (`temp_print_golden`, run once with `--nocapture`
/// and removed) and pins it against future drift.
#[test]
fn build_snapshot_bytes_for_ring_output_at_capacity_ten_thousand_matches_the_pinned_golden_bytes() {
    let cols = 10u16;
    let rows = 4u16;
    let mut stream = Vec::new();
    for i in 0..20u32 {
        stream.extend_from_slice(format!("L{i}\r\n").as_bytes());
    }
    let (raw, segments, wrapped, shadow_dump) =
        wrapped_ring_and_shadow_dump(cols, rows, 64, &stream);
    assert!(wrapped, "test prerequisite: the ring must have wrapped");

    let (payload, out_segments) = build_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        wrapped,
        (cols, rows),
        10_000,
    );

    let golden_payload: Vec<u8> = vec![
        27, 91, 51, 74, 27, 91, 72, 27, 91, 50, 74, 13, 10, 76, 55, 13, 10, 76, 56, 13, 10, 76, 57,
        13, 10, 76, 49, 48, 13, 10, 76, 49, 49, 13, 10, 76, 49, 50, 13, 10, 76, 49, 51, 13, 10, 76,
        49, 52, 13, 10, 76, 49, 53, 13, 10, 76, 49, 54, 13, 10, 76, 49, 55, 13, 10, 76, 49, 56, 13,
        10, 76, 49, 57, 13, 10, 27, 91, 63, 49, 48, 52, 57, 108, 27, 91, 63, 54, 108, 27, 91, 114,
        27, 91, 63, 50, 53, 104, 27, 91, 109, 27, 91, 72, 27, 91, 74, 76, 49, 55, 13, 10, 76, 49,
        56, 13, 10, 76, 49, 57, 13, 10, 27, 91, 49, 59, 52, 114, 27, 91, 63, 54, 108, 27, 91, 52,
        59, 49, 72, 27, 91, 48, 109,
    ];
    let golden_segments: Vec<(usize, u16, u16)> = vec![(0, 10, 4), (83, 10, 4)];

    assert_eq!(
        payload, golden_payload,
        "payload drifted from the pinned golden output"
    );
    assert_eq!(
        out_segments, golden_segments,
        "segments drifted from the pinned golden output"
    );
}
