use super::*;

// ── process_pty_data interruptible tests ─────────────

#[test]
fn test_process_pty_data_normal_consumes_all() {
    let mut core = TerminalCore::new(80, 24, 0);
    let data = b"Hello";
    let consumed = core.process_pty_data(data);
    assert_eq!(consumed, data.len());
    assert!(core.mode_actions.is_empty());
}

#[test]
fn test_reset_and_replay_paints_only_replay_bytes() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.process_pty_data(b"old data");
    // Sanity: first cell now holds 'o'.
    assert_eq!(core.get_cell_char(0, 0), "o");
    // Replay a different stream.
    core.reset_and_replay(b"NEW");
    assert_eq!(core.get_cell_char(0, 0), "N");
    assert_eq!(core.get_cell_char(1, 0), "E");
    assert_eq!(core.get_cell_char(2, 0), "W");
    // Cell 3 must be empty after reset (no leftover from "old data").
    assert_eq!(core.get_cell_char(3, 0), " ");
}

#[test]
fn test_reset_and_replay_empty_bytes_clears_grid() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.process_pty_data(b"junk");
    core.reset_and_replay(b"");
    assert_eq!(core.get_cell_char(0, 0), " ");
}

#[test]
fn test_process_pty_data_stops_on_buffer_switch() {
    let mut core = TerminalCore::new(80, 24, 0);
    // CSI ?1049h (8 bytes) followed by "AB"
    let data = b"\x1B[?1049hAB";
    let consumed = core.process_pty_data(data);
    assert_eq!(consumed, 8);
    assert!(core.has_pending_buffer_switch());
    // The mode action should be MODE_ACTION_SAVE_AND_SWITCH_TO_ALT (2)
    let actions = core.take_mode_actions();
    assert!(actions.contains(&2));
}

#[test]
fn test_has_pending_buffer_switch_empty() {
    let core = TerminalCore::new(80, 24, 0);
    assert!(!core.has_pending_buffer_switch());
}

#[test]
fn test_has_pending_buffer_switch_skips_ts_fallback() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Simulate TS_FALLBACK entry: [0xFF, lo, hi]
    core.mode_actions.push(0xFF);
    core.mode_actions.push(0x01);
    core.mode_actions.push(0x00);
    assert!(!core.has_pending_buffer_switch());
}

#[test]
fn test_has_pending_buffer_switch_detects_switch_to_alt() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.mode_actions.push(1); // SWITCH_TO_ALT
    assert!(core.has_pending_buffer_switch());
}

#[test]
fn test_has_pending_buffer_switch_detects_switch_to_main() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.mode_actions.push(3); // SWITCH_TO_MAIN
    assert!(core.has_pending_buffer_switch());
}

// ── SGR combined RGB through full parse pipeline ──────

#[test]
fn test_process_pty_data_sgr_combined_rgb_fg_bg() {
    // Full pipeline test: raw bytes → parser → CSI dispatch → SGR handler.
    // ESC[38;2;200;200;200;48;2;43;48;59m = 10 SGR params
    // Then print 'X' to commit cursor attrs to a cell.
    let mut core = TerminalCore::new(80, 24, 0);
    let data = b"\x1b[38;2;200;200;200;48;2;43;48;59mX";
    let consumed = core.process_pty_data(data);
    assert_eq!(consumed, data.len());
    // Cell at (0,0) should have the correct colors
    let fg = PackedColor::from_u32(core.get_cell_fg(0, 0));
    let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
    assert_eq!(fg, PackedColor::rgb(200, 200, 200));
    assert_eq!(
        bg,
        PackedColor::rgb(43, 48, 59),
        "bg should be rgb(43,48,59), not indexed(3)"
    );
}

// ── Grapheme buffer flush on non-Print dispatch ──────

#[test]
fn test_grapheme_buffer_flushed_before_csi_cursor_move() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Print emoji (gets buffered as Extended_Pictographic)
    // then CSI CUP to move cursor, then print 'A'
    // Emoji should be at position (0,0), not at the CUP destination
    let data = b"\xF0\x9F\x98\x80\x1B[3;5HA"; // 😀 \x1b[3;5H A
    core.process_pty_data(data);
    // 😀 should be at (0, 0)
    assert_eq!(core.get_cell_char(0, 0), "😀");
    assert_eq!(core.get_cell_width(0, 0), 2);
    // 'A' should be at (4, 2) [CUP row=3 col=5 → 0-indexed (2, 4)]
    assert_eq!(core.get_cell_char(4, 2), "A");
}

#[test]
fn test_grapheme_buffer_flushed_before_execute_cr() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Move cursor to col 10 first, print emoji then CR
    // Emoji should be at col 10 (flushed before CR), not lost
    let data = b"\x1B[1;11H\xF0\x9F\x98\x80\r"; // CUP(1,11) 😀 CR
    core.process_pty_data(data);
    // 😀 should be at (10, 0) with width 2
    assert_eq!(core.get_cell_char(10, 0), "😀");
    assert_eq!(core.get_cell_width(10, 0), 2);
    // After CR, cursor should be at col 0
    assert_eq!(core.get_cursor_col(), 0);
}

#[test]
fn test_grapheme_buffer_flushed_before_execute_lf() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Print emoji then LF then 'A'
    let data = b"\xF0\x9F\x98\x80\nA"; // 😀 LF A
    core.process_pty_data(data);
    // 😀 should be at (0, 0), width 2
    assert_eq!(core.get_cell_char(0, 0), "😀");
    assert_eq!(core.get_cell_width(0, 0), 2);
    // After LF, cursor moves to row 1 (col stays at 2 from emoji advance)
    // 'A' should be at (2, 1)
    assert_eq!(core.get_cell_char(2, 1), "A");
}

#[test]
fn test_grapheme_buffer_flushed_before_esc_dispatch() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Move to row 1 first so ESC M (Reverse Index) goes to row 0
    // Print emoji at row 1, then ESC M
    let data = b"\x1B[2;1H\xF0\x9F\x98\x80\x1BM"; // CUP(2,1) 😀 ESC_M
    core.process_pty_data(data);
    // 😀 should be at (0, 1) — row 1, col 0
    assert_eq!(core.get_cell_char(0, 1), "😀");
    assert_eq!(core.get_cell_width(0, 1), 2);
    // After ESC M (reverse index), cursor should be at row 0
    assert_eq!(core.get_cursor_row(), 0);
}

// ── DEC mode 1048 immediate save/restore ──────────────

#[test]
fn test_dec_1048_save_restore_immediate_in_data_stream() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Write "AB" at (0,0), save cursor (CSI ?1048h), move to (10,5),
    // write "CD", restore cursor (CSI ?1048l), write "EF"
    // "EF" should appear at (2,0) (where cursor was saved), not at (12,5)
    let data = b"AB\x1B[?1048h\x1B[6;11HCD\x1B[?1048lEF";
    core.process_pty_data(data);
    // "AB" at (0,0) and (1,0)
    assert_eq!(core.get_cell_char(0, 0), "A");
    assert_eq!(core.get_cell_char(1, 0), "B");
    // "CD" at (10,5) and (11,5)
    assert_eq!(core.get_cell_char(10, 5), "C");
    assert_eq!(core.get_cell_char(11, 5), "D");
    // "EF" at (2,0) and (3,0) (restored cursor position)
    assert_eq!(core.get_cell_char(2, 0), "E");
    assert_eq!(core.get_cell_char(3, 0), "F");
    // No mode actions should be queued (handled immediately)
    assert!(core.mode_actions.is_empty());
}

#[test]
fn test_dec_1048_and_esc7_share_same_saved_cursor() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Save with ESC 7 at (5,3), move, restore with CSI ?1048l
    // They should share the same saved cursor slot
    core.set_cursor(5, 3);
    let data = b"\x1B7\x1B[10;20HX\x1B[?1048l";
    core.process_pty_data(data);
    // Cursor should be restored to (5,3) from ESC 7 save
    assert_eq!(core.get_cursor_col(), 5);
    assert_eq!(core.get_cursor_row(), 3);
}

// ── Cell size propagation tests ──────────────────────

#[test]
fn test_cell_size_defaults() {
    let core = TerminalCore::new(80, 24, 0);
    assert_eq!(core.get_cell_width_px(), 8);
    assert_eq!(core.get_cell_height_px(), 16);
}

#[test]
fn test_cell_size_preserved_after_reset() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.set_cell_size_px(10, 20);
    core.reset();
    // Cell size is not reset (app-managed, not terminal state)
    assert_eq!(core.get_cell_width_px(), 10);
    assert_eq!(core.get_cell_height_px(), 20);
}

#[test]
fn test_xtwinops_cell_size_after_buffer_switch_defaults() {
    // Simulates the problem: a new alternate core starts with default 8x16
    // CSI 16t should return the default before cell size is set
    let mut core = TerminalCore::new(80, 24, 0);
    let len = core.handle_xtwinops_cell_size();
    assert!(len > 0);
    let bytes = core.get_response_bytes();
    assert_eq!(&bytes, b"\x1b[6;16;8t");
    // task0002 D5: `get_response_bytes` now peeks the ORDERED pending
    // store (everything since the last drain), not a single
    // overwritten slot — drain here so the second query below starts
    // from an empty store, matching how the real write-back sites use
    // `take_response` between parses.
    core.take_response();

    // After setting cell size, CSI 16t should return the new values
    core.set_cell_size_px(10, 20);
    core.handle_xtwinops_cell_size();
    let bytes = core.get_response_bytes();
    assert_eq!(&bytes, b"\x1b[6;20;10t");
}

// ── BCE (Background Color Erase) tests ──────────────

/// Helper: set cursor bg to green (indexed color 2)
fn set_cursor_bg_green(core: &mut TerminalCore) {
    core.set_cursor_bg(1, 2, 0, 0); // tag=1 (indexed), index=2 (green)
}

#[test]
fn test_bce_clear_line() {
    let mut core = TerminalCore::new(10, 3, 0);
    set_cursor_bg_green(&mut core);
    core.clear_line(0);
    for col in 0..10 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
        assert_eq!(
            bg,
            PackedColor::indexed(2),
            "col {col} should have green bg"
        );
    }
}

#[test]
fn test_bce_clear_line_range() {
    let mut core = TerminalCore::new(10, 3, 0);
    set_cursor_bg_green(&mut core);
    core.clear_line_range(0, 3, 7);
    for col in 3..7 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
        assert_eq!(
            bg,
            PackedColor::indexed(2),
            "col {col} should have green bg"
        );
    }
    // Cols outside range should still be default
    let bg0 = PackedColor::from_u32(core.get_cell_bg(0, 0));
    assert_eq!(bg0, PackedColor::DEFAULT);
    let bg9 = PackedColor::from_u32(core.get_cell_bg(9, 0));
    assert_eq!(bg9, PackedColor::DEFAULT);
}

#[test]
fn test_bce_default_bg_unchanged() {
    // When cursor.bg is DEFAULT, erased cells should have DEFAULT bg
    let mut core = TerminalCore::new(10, 3, 0);
    // cursor.bg is already DEFAULT
    core.clear_line(0);
    for col in 0..10 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
        assert_eq!(bg, PackedColor::DEFAULT);
    }
}

#[test]
fn test_bce_sgr_reset_then_erase() {
    let mut core = TerminalCore::new(10, 3, 0);
    // Set green bg
    set_cursor_bg_green(&mut core);
    // Reset cursor attrs (simulates ESC[0m)
    core.reset_cursor_attrs();
    core.clear_line(0);
    for col in 0..10 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
        assert_eq!(
            bg,
            PackedColor::DEFAULT,
            "After reset, bg should be DEFAULT"
        );
    }
}

#[test]
fn test_bce_256_color() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.set_cursor_bg(1, 196, 0, 0); // indexed color 196
    core.clear_line(0);
    let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
    assert_eq!(bg, PackedColor::indexed(196));
}

#[test]
fn test_bce_rgb_color() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.set_cursor_bg(2, 100, 200, 50); // RGB
    core.clear_line(0);
    let bg = PackedColor::from_u32(core.get_cell_bg(0, 0));
    assert_eq!(bg, PackedColor::rgb(100, 200, 50));
}

#[test]
fn test_bce_shift_rows_up() {
    let mut core = TerminalCore::new(10, 5, 0);
    for row in 0..5 {
        for col in 0..10 {
            core.set_cell_ascii(col, row, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
    }
    set_cursor_bg_green(&mut core);
    core.shift_rows_up(0, 4, 2);
    // Vacated bottom rows (3, 4) should have green bg
    for row in 3..5 {
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, row));
            assert_eq!(bg, PackedColor::indexed(2), "row {row} col {col}");
        }
    }
}

// ── SlimCell stats tests (FR11) ──────────────────────

#[test]
fn test_slim_cell_total_initial_zero() {
    let core = TerminalCore::new(80, 24, 100);
    assert_eq!(core.slim_cell_total(), 0);
}

#[test]
fn test_slim_cell_total_after_eviction() {
    let mut core = TerminalCore::new(10, 3, 5);
    for r in 0..3 {
        core.set_cell(0, r, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.scroll_up_internal(2); // 2 rows go to scrollback
    // Each scrollback row has 10 SlimCells.
    assert_eq!(core.slim_cell_total(), 20);
}

// ── BCE shift_rows_down test ────────────────────────

#[test]
fn test_bce_shift_rows_down() {
    let mut core = TerminalCore::new(10, 5, 0);
    for row in 0..5 {
        for col in 0..10 {
            core.set_cell_ascii(col, row, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
    }
    set_cursor_bg_green(&mut core);
    core.shift_rows_down(0, 4, 2);
    // Vacated top rows (0, 1) should have green bg
    for row in 0..2 {
        for col in 0..10 {
            let bg = PackedColor::from_u32(core.get_cell_bg(col, row));
            assert_eq!(bg, PackedColor::indexed(2), "row {row} col {col}");
        }
    }
}

// ── Reparse-cost measurement harness (FR1) ───────────
//
// A deterministic, on-demand timing harness that feeds a synthetic
// scrollback through `process_pty_data_fully` on a fresh core and reports
// the elapsed time + throughput. The synthetic input is fixed (no RNG, no
// clock) so re-runs are stable, and the harness calls `term_core` directly
// (no `App::pump_all`, no real PTY) so it is isolated from the flaky pump
// path. The timing test is `#[ignore]`-gated so the default `cargo test`
// run is unaffected; run it with `cargo test -- --ignored --nocapture`.

/// Build a deterministic, terminal-representative byte buffer of about
/// `target_bytes` bytes. The content mixes printable ASCII text, periodic
/// newlines (so the parser scrolls and fills scrollback), and an occasional
/// SGR colour change — no RNG and no clock input, so the buffer is
/// byte-for-byte reproducible across runs and machines.
fn build_synthetic_scrollback(target_bytes: usize) -> Vec<u8> {
    // A short, fixed palette of SGR colour changes cycled deterministically.
    const SGRS: &[&[u8]] = &[
        b"\x1b[31m", // red
        b"\x1b[32m", // green
        b"\x1b[33m", // yellow
        b"\x1b[34m", // blue
        b"\x1b[0m",  // reset
    ];
    // Printable glyphs cycled per column (deterministic, ASCII only).
    const GLYPHS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789 ";

    let mut out: Vec<u8> = Vec::with_capacity(target_bytes + 64);
    let mut col: usize = 0;
    let mut line: usize = 0;
    let mut glyph_i: usize = 0;
    // Wrap-ish line width so newlines come periodically (~80 cols).
    const LINE_WIDTH: usize = 78;

    while out.len() < target_bytes {
        if col == 0 {
            // Once every 8 lines, emit a deterministic SGR change so the
            // stream exercises the colour path without dominating it.
            if line % 8 == 0 {
                out.extend_from_slice(SGRS[(line / 8) % SGRS.len()]);
            }
        }
        out.push(GLYPHS[glyph_i % GLYPHS.len()]);
        glyph_i += 1;
        col += 1;
        if col >= LINE_WIDTH {
            out.push(b'\r');
            out.push(b'\n');
            col = 0;
            line += 1;
        }
    }
    out
}

#[test]
fn test_synthetic_scrollback_is_deterministic() {
    // Same size in -> byte-identical buffer out (no RNG / clock).
    let a = build_synthetic_scrollback(64 * 1024);
    let b = build_synthetic_scrollback(64 * 1024);
    assert_eq!(a, b, "synthetic scrollback must be reproducible");
    assert!(
        a.len() >= 64 * 1024,
        "buffer should reach the requested size"
    );
    // Sanity: it contains newlines and at least one SGR introducer.
    assert!(a.contains(&b'\n'), "should contain newlines");
    assert!(
        a.windows(2).any(|w| w == b"\x1b["),
        "should contain SGR sequences"
    );
}

#[test]
fn test_reparse_empty_input_no_panic() {
    // FR1 empty-input guard: feeding 0 bytes through the full-drain reparse
    // path neither panics nor misreports; elapsed time is ~0 ms.
    let mut core = TerminalCore::new(80, 24, 10_000);
    let start = std::time::Instant::now();
    let actions = core.process_pty_data_fully(b"");
    let elapsed = start.elapsed();
    assert!(actions.is_empty(), "empty input yields no mode actions");
    // ~0 ms: be generous to avoid flakiness on loaded CI, but it must not
    // wander into the tens of ms a real reparse would take.
    assert!(
        elapsed.as_millis() < 50,
        "empty reparse should be ~0 ms, was {:?}",
        elapsed
    );
}

/// Gated measurement harness (FR1 -> FR2). Excluded from the default
/// `cargo test` run via `#[ignore]`. Run explicitly with:
///
/// ```text
/// cargo test -p term_core -- --ignored --nocapture
/// ```
///
/// Reports the reparse time + throughput for a ~2 MiB synthetic scrollback,
/// plus a few smaller sizes to show scaling. The measured ~2 MiB figure is
/// the input to the §4 threshold decision recorded at verify time.
#[test]
#[ignore = "measurement harness; run with --ignored --nocapture"]
fn measure_reparse_cost_2mib() {
    // Sizes: 256 KiB / 1 MiB / 2 MiB so scaling is visible. The 2 MiB run
    // is the headline figure for the go/no-go decision.
    const SIZES: &[(usize, &str)] = &[
        (256 * 1024, "256 KiB"),
        (1024 * 1024, "1 MiB"),
        (2 * 1024 * 1024, "2 MiB"),
    ];

    eprintln!("=== reparse-cost measurement (process_pty_data_fully) ===");
    for &(size, label) in SIZES {
        let buf = build_synthetic_scrollback(size);
        // Fresh core at a representative grid size with a 2 MiB-ish
        // scrollback capacity so rows actually accumulate.
        let mut core = TerminalCore::new(80, 24, 50_000);

        let start = std::time::Instant::now();
        let _ = core.process_pty_data_fully(&buf);
        let elapsed = start.elapsed();

        let bytes = buf.len() as f64;
        let secs = elapsed.as_secs_f64();
        let mib = bytes / (1024.0 * 1024.0);
        let mibps = if secs > 0.0 {
            mib / secs
        } else {
            f64::INFINITY
        };
        eprintln!(
            "{label:>8}: {bytes:>9.0} bytes  {ms:>8.3} ms  {mibps:>8.1} MiB/s",
            bytes = bytes,
            ms = elapsed.as_secs_f64() * 1000.0,
            mibps = mibps,
        );
    }
    eprintln!("=========================================================");
}
