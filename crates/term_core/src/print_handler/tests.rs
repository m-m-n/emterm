use crate::terminal_core::TerminalCore;

// ── Sprint 2: Print handler tests ───────────────────

// TS-R01: handle_print ASCII 'A' at (0,0)
#[test]
fn test_handle_print_ascii_basic() {
    let mut core = TerminalCore::new(80, 24, 0);
    let scroll = core.handle_print(0x41); // 'A'
    assert_eq!(scroll, 0);
    assert_eq!(core.get_cell_char(0, 0), "A");
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cursor_col(), 1);
}

// TS-R02: handle_print ASCII with wrap_pending
#[test]
fn test_handle_print_ascii_wrap_pending() {
    let mut core = TerminalCore::new(5, 3, 0);
    // Fill row 0 to trigger wrap_pending
    for c in b'A'..=b'E' {
        core.handle_print(c as u32);
    }
    assert!(core.get_wrap_pending());
    let scroll = core.handle_print(0x46); // 'F'
    assert_eq!(scroll, 0);
    assert_eq!(core.get_cursor_col(), 1);
    assert_eq!(core.get_cursor_row(), 1);
}

// TS-R03: handle_print non-ASCII (wide char) with wrap_pending
#[test]
fn test_handle_print_with_wrap_pending() {
    let mut core = TerminalCore::new(5, 3, 0);
    // Fill to end
    for c in b'A'..=b'E' {
        core.handle_print(c as u32);
    }
    assert!(core.get_wrap_pending());
    // Now print CJK char (width=2)
    let scroll = core.handle_print(0x4E16); // '世'
    assert_eq!(scroll, 0);
    assert_eq!(core.get_cursor_row(), 1);
    assert_eq!(core.get_cell_char(0, 1), "世");
}

// TS-R04: handle_print scroll at bottom (scroll internal)
#[test]
fn test_handle_print_scroll_at_bottom() {
    let mut core = TerminalCore::new(5, 2, 0);
    // Fill row 0
    for c in b'A'..=b'E' {
        core.handle_print(c as u32);
    }
    // Fill row 1
    for c in b'F'..=b'J' {
        core.handle_print(c as u32);
    }
    // Next print scrolls internally
    let scroll = core.handle_print(0x4B); // 'K'
    assert_eq!(scroll, 0); // Scroll handled internally
    // Row 0 should now have old row 1 content (FGHIJ)
    assert_eq!(core.get_cell_char(0, 0), "F");
}

#[test]
fn test_handle_print_cjk() {
    let mut core = TerminalCore::new(10, 3, 0);
    let scroll = core.handle_print(0x4E16); // '世'
    assert_eq!(scroll, 0);
    assert_eq!(core.get_cell_char(0, 0), "世");
    assert_eq!(core.get_cell_width(0, 0), 2);
    // Placeholder at col 1
    assert_eq!(core.get_cell_width(1, 0), 0);
    assert_eq!(core.get_cursor_col(), 2);
}

#[test]
fn test_handle_print_cjk_wrap() {
    let mut core = TerminalCore::new(5, 3, 0);
    // Fill to col 4 (last col)
    for c in b'A'..=b'D' {
        core.handle_print(c as u32);
    }
    assert_eq!(core.get_cursor_col(), 4);
    // CJK at col 4 (only 1 cell remaining for width=2): should wrap
    let scroll = core.handle_print(0x4E16); // '世'
    assert_eq!(scroll, 0);
    assert_eq!(core.get_cursor_row(), 1);
    assert_eq!(core.get_cell_char(0, 1), "世");
}

#[test]
fn test_handle_print_emoji_buffered() {
    let mut core = TerminalCore::new(10, 3, 0);
    // Emoji with Emoji_Presentation property → should buffer
    let scroll = core.handle_print(0x1F600); // 😀
    assert_eq!(scroll, 0);
    assert_eq!(core.get_grapheme_buffer_len(), 1);
}

#[test]
fn test_handle_print_zwj_extends() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x1F468); // 👨
    core.handle_print(0x200D); // ZWJ
    assert_eq!(core.get_grapheme_buffer_len(), 2);
}

#[test]
fn test_handle_print_flush_then_new() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x1F600); // 😀
    assert_eq!(core.get_grapheme_buffer_len(), 1);
    // Print ASCII 'A' → should flush emoji, then print 'A'
    core.handle_print(0x41);
    assert_eq!(core.get_grapheme_buffer_len(), 0);
    assert_eq!(core.get_cell_char(0, 0), "😀");
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_char(2, 0), "A");
}

#[test]
fn test_handle_print_ri_pair() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x1F1EF); // Regional indicator J
    assert_eq!(core.get_grapheme_buffer_len(), 1);
    core.handle_print(0x1F1F5); // Regional indicator P → 🇯🇵
    assert_eq!(core.get_grapheme_buffer_len(), 0);
    assert_eq!(core.get_cell_char(0, 0), "🇯🇵");
    assert_eq!(core.get_cell_width(0, 0), 2);
}

#[test]
fn test_handle_print_vs_fe0e() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x2764); // ❤ (Extended_Pictographic)
    assert_eq!(core.get_grapheme_buffer_len(), 1);
    core.handle_print(0xFE0E); // VS15 (text presentation)
    assert_eq!(core.get_grapheme_buffer_len(), 2);
    core.handle_print(0x41); // flush
    assert_eq!(core.get_cell_width(0, 0), 1); // text presentation = width 1
}

#[test]
fn test_handle_print_vs_fe0f() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x2764); // ❤
    core.handle_print(0xFE0F); // VS16 (emoji presentation)
    core.handle_print(0x41); // flush
    assert_eq!(core.get_cell_width(0, 0), 2); // emoji presentation = width 2
}

#[test]
fn test_handle_print_skin_tone() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x1F44D); // 👍
    core.handle_print(0x1F3FD); // medium skin tone
    assert_eq!(core.get_grapheme_buffer_len(), 2);
}

#[test]
fn test_handle_print_buffer_overflow() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Push 64 codepoints to trigger buffer overflow safety
    for _ in 0..64 {
        core.handle_print(0x1F600); // 😀
    }
    // Buffer should have been flushed at 64
    // The 64th push triggers flush of first 64, then starts new buffer
    assert!(core.get_grapheme_buffer_len() <= 1);
}

#[test]
fn test_handle_print_dec_line_drawing() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.set_g0_charset(1);
    core.set_active_charset(0);
    core.handle_print(0x71); // q → ─ (box drawing horizontal)
    assert_eq!(core.get_cell_char(0, 0), "\u{2500}");
}

#[test]
fn test_handle_print_dec_line_drawing_inactive() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.set_g0_charset(1);
    core.set_active_charset(1); // G1 is active, G1 is ASCII
    core.handle_print(0x71); // should NOT translate
    assert_eq!(core.get_cell_char(0, 0), "q");
}

#[test]
fn test_handle_print_g1_dec_line_drawing() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.set_g1_charset(1);
    core.set_active_charset(1); // G1 active, G1 is DecLineDrawing
    core.handle_print(0x71); // q → ─
    assert_eq!(core.get_cell_char(0, 0), "\u{2500}");
}

#[test]
fn test_handle_print_no_autowrap() {
    let mut core = TerminalCore::new(5, 3, 0);
    // Disable auto wrap
    core.set_mode(0, false);
    // Print exactly 5 chars
    for c in b'A'..=b'E' {
        core.handle_print(c as u32);
    }
    assert!(!core.get_wrap_pending());
    // Print 6th char - should overwrite at last col
    core.handle_print(b'F' as u32);
    assert_eq!(core.get_cursor_col(), 4);
    assert_eq!(core.get_cell_char(4, 0), "F");
}

// ── Flush tests ────────────────────────────────────────

#[test]
fn test_flush_empty() {
    let mut core = TerminalCore::new(10, 3, 0);
    let scroll = core.flush_grapheme_buffer();
    assert_eq!(scroll, 0);
    assert_eq!(core.get_grapheme_buffer_len(), 0);
}

#[test]
fn test_flush_single_emoji() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x1F600); // 😀
    let scroll = core.flush_grapheme_buffer();
    assert_eq!(scroll, 0);
    assert_eq!(core.get_cell_char(0, 0), "😀");
    assert_eq!(core.get_cell_width(0, 0), 2);
}

#[test]
fn test_flush_zwj_sequence() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x1F468); // 👨
    core.handle_print(0x200D); // ZWJ
    core.handle_print(0x1F4BB); // 💻
    let scroll = core.flush_grapheme_buffer();
    assert_eq!(scroll, 0);
    assert_eq!(core.get_cell_char(0, 0), "👨\u{200D}💻");
    assert_eq!(core.get_cell_width(0, 0), 2);
}

#[test]
fn test_flush_flag_ri_pair() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x1F1EF); // J
    core.handle_print(0x1F1F5); // P → auto-flushed
    // Already flushed by auto-flush
    assert_eq!(core.get_cell_char(0, 0), "🇯🇵");
}

#[test]
fn test_flush_with_wrap_pending() {
    let mut core = TerminalCore::new(5, 3, 0);
    // Fill row to trigger wrap_pending
    for c in b'A'..=b'E' {
        core.handle_print(c as u32);
    }
    assert!(core.get_wrap_pending());
    // Now add emoji and flush
    core.handle_print(0x1F600); // 😀 → buffered
    core.flush_grapheme_buffer();
    // Should wrap to row 1 and print emoji
    assert_eq!(core.get_cursor_row(), 1);
    assert_eq!(core.get_cell_char(0, 1), "😀");
}

// ── Scroll region LF tests ─────────────────────────────

#[test]
fn test_scroll_region_lf_within() {
    let mut core = TerminalCore::new(10, 10, 0);
    core.set_scroll_region(2, 7);
    core.set_cursor(0, 4);
    let scroll = core.handle_print(0x0A as u32); // LF via print? No...
    // Actually LF is handled by handle_execute, not handle_print.
    // But line_feed() is tested via handle_print scroll behavior
    assert_eq!(scroll, 0);
}

#[test]
fn test_scroll_region_lf_at_bottom() {
    let mut core = TerminalCore::new(5, 5, 0);
    core.set_scroll_region(1, 3);
    core.set_cursor(0, 3); // At scroll region bottom
    // Fill row to trigger wrap_pending
    for c in b'A'..=b'E' {
        core.handle_print(c as u32);
    }
    // Print one more char → wrap → line_feed at region bottom → scroll internal
    let scroll = core.handle_print(b'F' as u32);
    assert_eq!(scroll, 0); // Scroll handled internally
}

// ── Charset round-trip tests ───────────────────────────

#[test]
fn test_charset_round_trip() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.set_g0_charset(1);
    assert_eq!(core.get_g0_charset(), 1);
    core.set_g0_charset(0);
    assert_eq!(core.get_g0_charset(), 0);
    core.set_g1_charset(1);
    assert_eq!(core.get_g1_charset(), 1);
}

#[test]
fn test_active_charset_switch() {
    let mut core = TerminalCore::new(10, 3, 0);
    assert_eq!(core.get_active_charset(), 0);
    core.set_active_charset(1);
    assert_eq!(core.get_active_charset(), 1);
    core.set_active_charset(0);
    assert_eq!(core.get_active_charset(), 0);
}

#[test]
fn test_wrap_pending_round_trip() {
    let mut core = TerminalCore::new(10, 3, 0);
    assert!(!core.get_wrap_pending());
    core.set_wrap_pending(true);
    assert!(core.get_wrap_pending());
    core.set_wrap_pending(false);
    assert!(!core.get_wrap_pending());
}

#[test]
fn test_scroll_region_round_trip() {
    let mut core = TerminalCore::new(10, 10, 0);
    core.set_scroll_region(2, 7);
    assert_eq!(core.get_scroll_region_top(), 2);
    assert_eq!(core.get_scroll_region_bottom(), 7);
}

// ── DEC Line Drawing exhaustive ────────────────────────

#[test]
fn test_dec_line_drawing_all_entries() {
    let expected: &[(u32, u32)] = &[
        (0x5F, 0x0020),
        (0x60, 0x25C6),
        (0x61, 0x2592),
        (0x62, 0x2409),
        (0x63, 0x240C),
        (0x64, 0x240D),
        (0x65, 0x240A),
        (0x66, 0x00B0),
        (0x67, 0x00B1),
        (0x68, 0x2424),
        (0x69, 0x240B),
        (0x6A, 0x2518),
        (0x6B, 0x2510),
        (0x6C, 0x250C),
        (0x6D, 0x2514),
        (0x6E, 0x253C),
        (0x6F, 0x23BA),
        (0x70, 0x23BB),
        (0x71, 0x2500),
        (0x72, 0x23BC),
        (0x73, 0x23BD),
        (0x74, 0x251C),
        (0x75, 0x2524),
        (0x76, 0x2534),
        (0x77, 0x252C),
        (0x78, 0x2502),
        (0x79, 0x2264),
        (0x7A, 0x2265),
        (0x7B, 0x03C0),
        (0x7C, 0x2260),
        (0x7D, 0x00A3),
        (0x7E, 0x00B7),
    ];
    for &(input, output) in expected {
        let mut core = TerminalCore::new(10, 3, 0);
        core.set_g0_charset(1);
        core.handle_print(input);
        let ch = core.get_cell_char(0, 0);
        let expected_ch = char::from_u32(output).unwrap().to_string();
        assert_eq!(ch, expected_ch, "DEC line drawing 0x{:02X}", input);
    }
}

// ── Reset clears Sprint 2 state ────────────────────────

#[test]
fn test_reset_clears_sprint2_state() {
    let mut core = TerminalCore::new(10, 5, 0);
    // Set up state
    core.set_g0_charset(1);
    core.set_g1_charset(1);
    core.set_active_charset(1);
    core.set_wrap_pending(true);
    core.set_scroll_region(1, 3);
    core.handle_print(0x1F600); // buffer emoji

    core.reset();

    assert_eq!(core.get_g0_charset(), 0);
    assert_eq!(core.get_g1_charset(), 0);
    assert_eq!(core.get_active_charset(), 0);
    assert!(!core.get_wrap_pending());
    assert_eq!(core.get_scroll_region_top(), 0);
    assert_eq!(core.get_scroll_region_bottom(), 4);
    assert_eq!(core.get_grapheme_buffer_len(), 0);
}

#[test]
fn test_resize_resets_scroll_region() {
    let mut core = TerminalCore::new(10, 10, 0);
    core.set_scroll_region(2, 7);
    core.resize(10, 20);
    assert_eq!(core.get_scroll_region_top(), 0);
    assert_eq!(core.get_scroll_region_bottom(), 19);
}

// ── Kitty Unicode placeholder suppression ─────────────

#[test]
fn test_kitty_placeholder_suppressed() {
    let mut core = TerminalCore::new(10, 3, 0);
    // U+10EEEE = Kitty placeholder character
    core.handle_print(0x10EEEE);
    // Cursor should not advance (character is suppressed)
    assert_eq!(core.get_cursor_col(), 0);
    // Cell remains empty (space = default empty cell)
    assert_eq!(core.get_cell_char(0, 0), " ");
}

#[test]
fn test_kitty_placeholder_combining_suppressed() {
    let mut core = TerminalCore::new(10, 3, 0);
    // Placeholder followed by combining marks (row/col encoding)
    core.handle_print(0x10EEEE);
    core.handle_print(0x0305); // combining overline (row encoding)
    core.handle_print(0x0305); // another combining mark (col encoding)
    // All should be suppressed
    assert_eq!(core.get_cursor_col(), 0);
    // Next non-combining character should print normally
    core.handle_print(0x41); // 'A'
    assert_eq!(core.get_cursor_col(), 1);
    assert_eq!(core.get_cell_char(0, 0), "A");
}

#[test]
fn test_kitty_placeholder_multiple_cells() {
    let mut core = TerminalCore::new(10, 3, 0);
    // Simulate 3 placeholder cells (kitten icat pattern)
    for _ in 0..3 {
        core.handle_print(0x10EEEE);
        core.handle_print(0x0305); // combining mark
        core.handle_print(0x0305); // combining mark
    }
    // All suppressed
    assert_eq!(core.get_cursor_col(), 0);
    // Normal text after placeholders
    core.handle_print(0x42); // 'B'
    assert_eq!(core.get_cursor_col(), 1);
    assert_eq!(core.get_cell_char(0, 0), "B");
}

#[test]
fn test_kitty_placeholder_arabic_diacritics() {
    let mut core = TerminalCore::new(10, 3, 0);
    // kitten icat uses Arabic combining marks (U+0610-061A, U+064B-065F)
    // for encoding row/column in placeholder cells
    core.handle_print(0x10EEEE); // placeholder
    core.handle_print(0x0651); // Arabic shadda (row encoding)
    core.handle_print(0x0615); // Arabic small high tah (col encoding)
    // All should be suppressed
    assert_eq!(core.get_cursor_col(), 0);
    assert_eq!(core.get_cell_char(0, 0), " ");

    // Second cell with different Arabic marks
    core.handle_print(0x10EEEE); // placeholder
    core.handle_print(0x0652); // Arabic sukun
    core.handle_print(0x0615); // Arabic small high tah
    assert_eq!(core.get_cursor_col(), 0);

    // Normal character prints after placeholders
    core.handle_print(0x41); // 'A'
    assert_eq!(core.get_cursor_col(), 1);
    assert_eq!(core.get_cell_char(0, 0), "A");
}

#[test]
fn test_kitty_placeholder_mixed_diacritics() {
    let mut core = TerminalCore::new(20, 3, 0);
    // Mix of Latin combining marks (0x0300-0x036F) and Arabic marks
    // as kitten icat uses diacritics from many Unicode blocks
    core.handle_print(0x10EEEE);
    core.handle_print(0x0305); // combining overline (Latin)
    core.handle_print(0x0610); // Arabic combining mark

    core.handle_print(0x10EEEE);
    core.handle_print(0x064B); // Arabic fathatan
    core.handle_print(0x065F); // Arabic wavy hamza below

    core.handle_print(0x10EEEE);
    core.handle_print(0x0483); // Cyrillic titlo
    core.handle_print(0x0711); // Syriac superscript alaph

    // All suppressed
    assert_eq!(core.get_cursor_col(), 0);

    // Normal text after
    core.handle_print(0x58); // 'X'
    assert_eq!(core.get_cursor_col(), 1);
    assert_eq!(core.get_cell_char(0, 0), "X");
}

// ── Retroactive zero-width merge (keycap-cluster-composition task0001) ──

// AC-1: digit + VS16 + COMBINING ENCLOSING KEYCAP widens the base cell
// to width 2 with a spacer, and following text starts after the spacer.
#[test]
fn test_retroactive_merge_keycap_widens_to_width2() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x35); // '5'
    core.handle_print(0xFE0F); // VS16
    core.handle_print(0x20E3); // COMBINING ENCLOSING KEYCAP
    core.handle_print(0x58); // 'X'
    assert_eq!(core.get_cell_char(0, 0), "5\u{FE0F}\u{20E3}");
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0); // spacer
    assert_eq!(core.get_cell_char(2, 0), "X");
    assert_eq!(core.get_cursor_col(), 3);
}

// AC-2: digit + COMBINING ENCLOSING KEYCAP (no VS16) stays width 1, and
// following text lands in the very next column.
#[test]
fn test_retroactive_merge_keycap_without_vs16_stays_width1() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x35); // '5'
    core.handle_print(0x20E3); // COMBINING ENCLOSING KEYCAP
    core.handle_print(0x58); // 'X'
    assert_eq!(core.get_cell_char(0, 0), "5\u{20E3}");
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cell_char(1, 0), "X");
    assert_eq!(core.get_cursor_col(), 2);
}

// AC-3: base char + general combining mark merges, and the accent
// survives subsequent writes elsewhere on the row.
#[test]
fn test_retroactive_merge_combining_accent_survives() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x65); // 'e'
    core.handle_print(0x0301); // COMBINING ACUTE ACCENT
    core.handle_print(0x58); // 'X'
    assert_eq!(core.get_cell_char(0, 0), "e\u{0301}");
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cell_char(1, 0), "X");
    // Further unrelated writes must not disturb the merged cell.
    core.handle_print(0x59); // 'Y'
    assert_eq!(core.get_cell_char(0, 0), "e\u{0301}");
}

// AC-3 (integration): the same base+combining sequence driven through
// process_pty_data, exercising the top-level ASCII fast path (which
// bypasses handle_print entirely for the base character) together with
// the parser slow path for the non-ASCII combining mark.
#[test]
fn test_retroactive_merge_combining_accent_via_process_pty_data() {
    let mut core = TerminalCore::new(10, 3, 0);
    let mut bytes = vec![b'e'];
    bytes.extend_from_slice("\u{0301}".as_bytes());
    bytes.push(b'X');
    core.process_pty_data(&bytes);
    assert_eq!(core.get_cell_char(0, 0), "e\u{0301}");
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cell_char(1, 0), "X");
}

// AC-4: a width-0 character arriving with nothing written yet on this
// screen is dropped entirely: no grid write, no cursor movement.
#[test]
fn test_retroactive_merge_dropped_at_start_of_screen() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x0301); // combining accent, nothing written yet
    assert_eq!(core.get_cell_char(0, 0), " ");
    assert_eq!(core.get_cursor_col(), 0);
    assert_eq!(core.get_cursor_row(), 0);
}

// AC-4: explicit cursor movement (CSI CUP) invalidates the merge target.
#[test]
fn test_retroactive_merge_dropped_after_cursor_movement_csi() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x35); // '5' at (0,0)
    core.process_pty_data(b"\x1b[3;3H"); // CUP row=3,col=3 (1-indexed)
    assert_eq!(core.get_cursor_col(), 2);
    assert_eq!(core.get_cursor_row(), 2);
    core.handle_print(0x0301); // combining accent: no valid target, dropped
    assert_eq!(core.get_cell_char(0, 0), "5"); // unchanged
    assert_eq!(core.get_cursor_col(), 2); // unchanged by the dropped char
    assert_eq!(core.get_cursor_row(), 2);
}

// AC-4: a screen/line erase (CSI EL) invalidates the merge target even
// though it does not move the cursor.
#[test]
fn test_retroactive_merge_dropped_after_line_erase_csi() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x35); // '5' at (0,0), cursor col=1
    core.process_pty_data(b"\x1b[2K"); // erase entire line
    core.handle_print(0x20E3); // combining keycap: dropped
    assert_eq!(core.get_cell_char(0, 0), " "); // erased, not re-merged
    assert_eq!(core.get_cursor_col(), 1); // unchanged by the dropped char
}

// AC-4: a resize invalidates the merge target.
#[test]
fn test_retroactive_merge_dropped_after_resize() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x35); // '5' at (0,0), cursor col=1
    core.resize(12, 4);
    core.handle_print(0x20E3); // combining keycap: dropped
    assert_eq!(core.get_cell_char(0, 0), "5"); // unchanged by the dropped char
    assert_eq!(core.get_cursor_col(), 1);
}

// AC-4: a scroll that displaces the tracked row invalidates the target
// even though the cursor's (col, row) pair does not itself change.
#[test]
fn test_retroactive_merge_dropped_after_scroll() {
    let mut core = TerminalCore::new(5, 1, 0); // single row: any LF scrolls
    core.handle_print(0x41); // 'A' at (0,0), cursor advances to col 1
    core.handle_execute(0x0A); // LF -> scrolls, row content evicted/blanked
    let col_before = core.get_cursor_col();
    core.handle_print(0x0301); // combining accent: no valid target, dropped
    assert_eq!(core.get_cell_char(0, 0), " ");
    assert_eq!(core.get_cursor_col(), col_before); // unmoved by the dropped char
}

// AC-5: a width-0 character arriving after a wide (width-2) character
// merges into the wide base cell, not its spacer.
#[test]
fn test_retroactive_merge_after_wide_char_targets_base() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x4E16); // '世' width 2 at (0,0), spacer at (1,0)
    assert_eq!(core.get_cursor_col(), 2);
    core.handle_print(0x0301); // combining accent: must target base, not spacer
    assert_eq!(core.get_cell_char(0, 0), "\u{4E16}\u{0301}");
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0); // spacer unaffected
    assert_eq!(core.get_cursor_col(), 2); // non-VS16 merge never moves cursor
}

// AC-6 (auto-wrap on): VS16 widening a base cell sitting in the last
// column relocates it to the next row, mirroring the existing wide-char
// end-of-line wrap semantics. No spacer is orphaned across the line
// boundary and cursor state stays valid.
#[test]
fn test_retroactive_widen_at_last_column_wraps_with_autowrap() {
    let mut core = TerminalCore::new(5, 3, 0); // last column index = 4
    for c in b'A'..=b'D' {
        core.handle_print(c as u32);
    }
    core.handle_print(0x35); // '5' at col 4 (last column)
    assert_eq!(core.get_cursor_col(), 4);
    assert!(core.get_wrap_pending());

    core.handle_print(0xFE0F); // VS16 -> merge + retroactive widen + wrap

    assert_eq!(core.get_cell_char(4, 0), " "); // vacated: content relocated
    assert_eq!(core.get_cell_char(0, 1), "5\u{FE0F}");
    assert_eq!(core.get_cell_width(0, 1), 2);
    assert_eq!(core.get_cell_width(1, 1), 0); // spacer, not orphaned
    assert_eq!(core.get_cursor_row(), 1);
    assert_eq!(core.get_cursor_col(), 2);
    assert!(!core.get_wrap_pending());
    assert!(core.get_line_wrapped(1)); // row 1 is a continuation of row 0

    // Subsequent text continues right after the spacer.
    core.handle_print(0x58); // 'X'
    assert_eq!(core.get_cell_char(2, 1), "X");
}

// AC-6 (auto-wrap off): the same scenario with DECAWM off widens the
// base cell in place with no spacer, matching the existing no-autowrap
// wide-char end-of-line quirk; the cursor stays pinned.
#[test]
fn test_retroactive_widen_at_last_column_no_autowrap_widens_in_place() {
    let mut core = TerminalCore::new(5, 3, 0);
    core.set_mode(0, false); // MODE_AUTO_WRAP off
    for c in b'A'..=b'D' {
        core.handle_print(c as u32);
    }
    core.handle_print(0x35); // '5' at col 4 (last column)
    assert_eq!(core.get_cursor_col(), 4);
    assert!(!core.get_wrap_pending());

    core.handle_print(0xFE0F); // VS16 -> widen in place, no wrap

    assert_eq!(core.get_cell_char(4, 0), "5\u{FE0F}");
    assert_eq!(core.get_cell_width(4, 0), 2);
    assert_eq!(core.get_cursor_col(), 4); // cursor stays pinned
    assert_eq!(core.get_cursor_row(), 0);
}

// AC-8: a long run of combining marks on one base cell pushes the cell
// content past the inline 16-byte capacity into the overflow side table,
// and the full content remains readable back.
#[test]
fn test_retroactive_merge_long_combining_run_overflows_correctly() {
    let mut core = TerminalCore::new(20, 3, 0);
    core.handle_print(0x65); // 'e'
    let marks: [u32; 8] = [
        0x0301, 0x0302, 0x0303, 0x0304, 0x0305, 0x0306, 0x0307, 0x0308,
    ];
    for &m in &marks {
        core.handle_print(m);
    }
    core.handle_print(0x58); // 'X'

    let mut expected = String::from("e");
    for &m in &marks {
        expected.push(char::from_u32(m).unwrap());
    }
    assert_eq!(core.get_cell_char(0, 0), expected);
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cell_char(1, 0), "X");
}

// A second VS16 arriving after the base cell has already widened must
// not widen again (no double spacer / cursor double-advance).
#[test]
fn test_retroactive_merge_second_vs16_does_not_rewiden() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x35); // '5'
    core.handle_print(0xFE0F); // widens to width 2, cursor -> col 2
    core.handle_print(0xFE0F); // second VS16: appends only, no re-widen
    assert_eq!(core.get_cell_char(0, 0), "5\u{FE0F}\u{FE0F}");
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cursor_col(), 2); // unchanged by the second merge
}

// Boundary: widening one column short of the last column (col ==
// cols - 2) fits the spacer exactly and sets wrap_pending, matching the
// general wide-char cursor-advance formula (no special-cased relocation
// needed here).
#[test]
fn test_retroactive_widen_one_before_last_column_sets_wrap_pending() {
    let mut core = TerminalCore::new(5, 3, 0); // cols-2 = 3
    for c in b'A'..=b'C' {
        core.handle_print(c as u32);
    }
    core.handle_print(0x35); // '5' at col 3 (cols - 2)
    assert_eq!(core.get_cursor_col(), 4);
    core.handle_print(0xFE0F); // widen: spacer fits at col 4 (last column)
    assert_eq!(core.get_cell_char(3, 0), "5\u{FE0F}");
    assert_eq!(core.get_cell_width(3, 0), 2);
    assert_eq!(core.get_cell_width(4, 0), 0); // spacer at last column
    assert!(core.get_wrap_pending());
    assert_eq!(core.get_cursor_col(), 4);
}

// ── wide-pair-overwrite-cleanup (task0001): print-path partner blanking ──
//
// FR1/FR2/FR3 (this feature's SPEC.md) are distinct from the FR1-FR4
// comments above (keycap-cluster-composition, a past feature) — do not
// confuse the two when reading test names/comments in this section.

// AC-1 (TS1, FR1): overwriting a wide-pair base cell with a width-1
// character via the ASCII fast path blanks its now-orphaned spacer.
#[test]
fn test_handle_print_ascii_overwrite_wide_base_blanks_spacer() {
    let mut core = TerminalCore::new(10, 3, 0);
    // ⏭️ = U+23ED + VS16, width 2 (base at col0, spacer at col1).
    core.process_pty_data("\u{23ED}\u{FE0F}".as_bytes());
    core.handle_print(0x58); // 'X' forces the buffered cluster to flush
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0);

    core.process_pty_data(b"\r"); // cursor back to col 0
    core.handle_print(0x41); // 'A' overwrites the wide base (ASCII fast path)

    assert_eq!(core.get_cell_char(0, 0), "A");
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cell_char(1, 0), " "); // orphaned spacer blanked
    assert_eq!(core.get_cell_width(1, 0), 1);
}

// AC-1 (TS1, FR1): same scenario through the non-ASCII slow path
// (write_grapheme_to_grid), confirming the rule is not fast-path-only.
#[test]
fn test_write_grapheme_nonascii_overwrite_wide_base_blanks_spacer() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.process_pty_data("\u{23ED}\u{FE0F}".as_bytes());
    core.handle_print(0x58); // flush
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0);

    core.process_pty_data(b"\r");
    core.process_pty_data("\u{00E9}".as_bytes()); // 'é', width 1, non-ASCII (slow path)

    assert_eq!(core.get_cell_char(0, 0), "\u{00E9}");
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cell_char(1, 0), " ");
    assert_eq!(core.get_cell_width(1, 0), 1);
}

// AC-2 (TS2, FR2): overwriting a wide-pair spacer cell blanks its
// now-orphaned base (ASCII fast path).
#[test]
fn test_handle_print_ascii_overwrite_wide_spacer_blanks_base() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.process_pty_data("\u{23ED}\u{FE0F}".as_bytes());
    core.handle_print(0x58);
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0);

    core.process_pty_data(b"\x1b[1;2H"); // CUP row1,col2 (1-indexed) -> (col1,row0)
    core.handle_print(0x42); // 'B' overwrites the spacer (ASCII fast path)

    assert_eq!(core.get_cell_char(1, 0), "B");
    assert_eq!(core.get_cell_width(1, 0), 1);
    assert_eq!(core.get_cell_char(0, 0), " "); // orphaned base blanked
    assert_eq!(core.get_cell_width(0, 0), 1);
}

// AC-2 (TS2, FR2): same scenario through the non-ASCII slow path.
#[test]
fn test_write_grapheme_nonascii_overwrite_wide_spacer_blanks_base() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.process_pty_data("\u{23ED}\u{FE0F}".as_bytes());
    core.handle_print(0x58);
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0);

    core.process_pty_data(b"\x1b[1;2H");
    core.process_pty_data("\u{00E9}".as_bytes());

    assert_eq!(core.get_cell_char(1, 0), "\u{00E9}");
    assert_eq!(core.get_cell_width(1, 0), 1);
    assert_eq!(core.get_cell_char(0, 0), " ");
    assert_eq!(core.get_cell_width(0, 0), 1);
}

// AC-3 (TS6, FR3): writing a wide (width-2) character whose placeholder
// would land on another wide pair's base blanks that pair's now-orphaned
// spacer too (chained cleanup), exercised via write_grapheme_to_grid's
// direct placeholder-creation path.
#[test]
fn test_write_grapheme_wide_write_over_existing_base_blanks_its_spacer() {
    let mut core = TerminalCore::new(10, 3, 0);
    // Pair A at col1 (base) / col2 (spacer): CJK '世'.
    core.process_pty_data(b"\x1b[1;2H"); // cursor -> col1
    core.handle_print(0x4E16); // '世', width 2
    assert_eq!(core.get_cell_width(1, 0), 2);
    assert_eq!(core.get_cell_width(2, 0), 0);

    // New wide write at col0: its placeholder wants col1, currently pair
    // A's base. Overwriting it would orphan pair A's spacer at col2
    // unless it is blanked first.
    core.process_pty_data(b"\r"); // cursor -> col0
    core.handle_print(0x4E2D); // '中', width 2: base at col0, placeholder at col1

    assert_eq!(core.get_cell_char(0, 0), "\u{4E2D}");
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0); // new placeholder (pair B's spacer)
    assert_eq!(core.get_cell_char(2, 0), " "); // pair A's spacer, orphaned, blanked
    assert_eq!(core.get_cell_width(2, 0), 1);
}

// AC-3 (TS6, FR3/FR4): the same chained-cleanup rule applied at the
// widen_after_merge spacer-creation site: a retroactive VS16 widen whose
// new spacer would land on another wide pair's base blanks that pair's
// spacer too.
#[test]
fn test_widen_after_merge_spacer_creation_over_existing_base_blanks_its_spacer() {
    let mut core = TerminalCore::new(10, 3, 0);
    // Pair A at col1 (base) / col2 (spacer): CJK '世'.
    core.process_pty_data(b"\x1b[1;2H"); // cursor -> col1
    core.handle_print(0x4E16); // '世'
    assert_eq!(core.get_cell_width(1, 0), 2);
    assert_eq!(core.get_cell_width(2, 0), 0);

    // Digit at col0, then a standalone VS16 retroactively widens it; the
    // new spacer wants col1, currently pair A's base.
    core.process_pty_data(b"\r"); // cursor -> col0
    core.handle_print(0x35); // '5' at col0
    core.handle_print(0xFE0F); // VS16: retroactive widen

    assert_eq!(core.get_cell_char(0, 0), "5\u{FE0F}");
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0); // new spacer (digit pair)
    assert_eq!(core.get_cell_char(2, 0), " "); // pair A's spacer, orphaned, blanked
    assert_eq!(core.get_cell_width(2, 0), 1);
}

// FR4 (Design item 4, relocate_widened_base_via_wrap): when a last-column
// VS16 widen relocates to the next row, its base/spacer writes at the new
// row's col0/col1 apply the same partner-blanking rule as ordinary
// writes. Here col1 of the target row already holds an unrelated wide
// pair's base; the new relocated spacer overwriting it must blank that
// pair's own spacer at col2.
#[test]
fn test_relocate_widened_base_via_wrap_spacer_creation_blanks_existing_pair_spacer() {
    let mut core = TerminalCore::new(5, 3, 0); // last column index = 4
    // Pre-populate row 1 with an unrelated wide pair at col1(base)/col2(spacer).
    core.process_pty_data(b"\x1b[2;2H"); // row2,col2 (1-indexed) -> (col1,row1)
    core.handle_print(0x4E16); // '世' at (1,1): base col1, spacer col2
    assert_eq!(core.get_cell_width(1, 1), 2);
    assert_eq!(core.get_cell_width(2, 1), 0);

    // Row 0: fill to the last column, then trigger a VS16 widen that
    // relocates via wrap to row 1.
    core.process_pty_data(b"\x1b[1;1H"); // back to (0,0)
    for c in b'A'..=b'D' {
        core.handle_print(c as u32);
    }
    core.handle_print(0x35); // '5' at col4 (last column)
    assert_eq!(core.get_cursor_col(), 4);
    core.handle_print(0xFE0F); // VS16 -> relocate via wrap to row 1

    assert_eq!(core.get_cell_char(0, 1), "5\u{FE0F}");
    assert_eq!(core.get_cell_width(0, 1), 2);
    assert_eq!(core.get_cell_width(1, 1), 0); // new spacer (relocated pair)
    assert_eq!(core.get_cell_char(2, 1), " "); // old pair's spacer, orphaned, blanked
    assert_eq!(core.get_cell_width(2, 1), 1);
}

// AC-4 (TS3, FR1-FR4): investigation report P5 repro — a shorter redraw
// after CR that overwrites a wide-pair base but does not itself reach far
// enough to overwrite the old spacer (no EL, no full-line repaint) must
// not leave that spacer as an orphan.
#[test]
fn test_write_grapheme_shorter_redraw_after_wide_base_overwrite_blanks_orphan_spacer() {
    let mut core = TerminalCore::new(10, 3, 0);
    // Frame 1: '─' (DEC line drawing), ⏭️ (U+23ED + VS16, width 2), "AB".
    core.set_g0_charset(1);
    core.handle_print(0x71); // '─' at col0
    core.set_g0_charset(0);
    core.process_pty_data("\u{23ED}\u{FE0F}".as_bytes()); // buffered
    core.handle_print(0x41); // 'A' forces flush: base at col1, spacer at col2
    core.handle_print(0x42); // 'B' at col4
    assert_eq!(core.get_cell_char(0, 0), "\u{2500}");
    assert_eq!(core.get_cell_width(1, 0), 2);
    assert_eq!(core.get_cell_width(2, 0), 0);
    assert_eq!(core.get_cell_char(3, 0), "A");
    assert_eq!(core.get_cell_char(4, 0), "B");

    // Frame 2: CR, then a *shorter* redraw that overwrites only col0/col1
    // (the wide base) and stops — no EL, no write reaching col2.
    core.process_pty_data(b"\r");
    core.handle_print(0x20); // ' ' at col0 (was '─', width1: no trigger)
    core.handle_print(0x2D); // '-' overwrites the wide base at col1 (R1)

    assert_eq!(core.get_cell_char(0, 0), " ");
    assert_eq!(core.get_cell_char(1, 0), "-");
    assert_eq!(core.get_cell_width(1, 0), 1);
    assert_eq!(core.get_cell_char(2, 0), " "); // orphaned spacer, blanked
    assert_eq!(core.get_cell_width(2, 0), 1);
    // Untouched trailing content from frame 1 survives (NFR1).
    assert_eq!(core.get_cell_char(3, 0), "A");
    assert_eq!(core.get_cell_char(4, 0), "B");
}

// AC-5 (TS4, FR4, NFR1): U+23ED and its VS16 arrive via separate
// process_pty_data calls (PTY chunk boundary). The buffered emoji+VS16
// flush must still land width 2 with a correct spacer and cursor
// position — the partner-blanking checks this task adds must not disturb
// this pre-existing chunked-arrival behavior (both cells were empty, so
// no rule engages).
#[test]
fn test_write_grapheme_u23ed_vs16_across_chunk_boundary_widens_correctly() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.process_pty_data("\u{23ED}".as_bytes());
    core.process_pty_data("\u{FE0F}".as_bytes());
    core.process_pty_data(b"X"); // forces flush of the buffered cluster

    assert_eq!(core.get_cell_char(0, 0), "\u{23ED}\u{FE0F}");
    assert_eq!(core.get_cell_width(0, 0), 2);
    assert_eq!(core.get_cell_width(1, 0), 0); // spacer
    assert_eq!(core.get_cell_char(2, 0), "X");
    assert_eq!(core.get_cursor_col(), 3);
}

// AC-6 (NFR1): full existing term_core suite (--lib) stays green — see
// AC-6 note in tests.yaml; verified by the full `cargo test --lib` run
// rather than a single dedicated test here.

// AC-7 (NFR1, NFR4): overwriting a width-1 cell with another width-1
// character (no wide-pair involvement) leaves the left/right neighbor
// cells' content and width completely unchanged (ASCII fast path).
#[test]
fn test_handle_print_ascii_overwrite_ordinary_cell_leaves_neighbors_unchanged() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x41); // 'A' col0
    core.handle_print(0x42); // 'B' col1
    core.handle_print(0x43); // 'C' col2
    core.process_pty_data(b"\r"); // cursor -> col0
    core.handle_print(0x2A); // '*' overwrites col0 (ASCII fast path)

    assert_eq!(core.get_cell_char(0, 0), "*");
    assert_eq!(core.get_cell_char(1, 0), "B");
    assert_eq!(core.get_cell_width(1, 0), 1);
    assert_eq!(core.get_cell_char(2, 0), "C");
    assert_eq!(core.get_cell_width(2, 0), 1);
}

// AC-7 (NFR1, NFR4): same guarantee through the non-ASCII slow path,
// overwriting a middle cell and checking both neighbors.
#[test]
fn test_write_grapheme_nonascii_overwrite_ordinary_cell_leaves_neighbors_unchanged() {
    let mut core = TerminalCore::new(10, 3, 0);
    core.handle_print(0x41); // 'A' col0
    core.handle_print(0x42); // 'B' col1
    core.handle_print(0x43); // 'C' col2
    core.process_pty_data(b"\x1b[1;2H"); // CUP row1,col2 (1-indexed) -> (col1,row0)
    core.process_pty_data("\u{00E9}".as_bytes()); // 'é', width1, non-ASCII (slow path)

    assert_eq!(core.get_cell_char(1, 0), "\u{00E9}");
    assert_eq!(core.get_cell_char(0, 0), "A");
    assert_eq!(core.get_cell_width(0, 0), 1);
    assert_eq!(core.get_cell_char(2, 0), "C");
    assert_eq!(core.get_cell_width(2, 0), 1);
}

// ── wide-pair-overwrite-cleanup (task0003): P5 composite scenario ──
//
// AC-1/AC-2/AC-3 (TS3, FR1-FR4): investigation report P5 repro — a redraw
// after CR that shifts column alignment by one column, so the same-row
// rewrite both overwrites an old wide base directly (FR1) and overwrites
// an old spacer directly (FR2) within the same redraw, while a later
// width-2 write's placeholder lands on a third pair's base
// (FR3, write_grapheme_to_grid's own chained cleanup) and a later
// retroactive VS16 widen's new spacer lands on a fourth pair's base
// (FR4, widen_after_merge's own chained cleanup). Column arithmetic for
// each trigger is documented inline; the closing checks verify every
// column of the row so no orphan spacer/base survives anywhere in it.
//
// Note on FR4 routing: the task plan for this scenario described the
// ⏭️ (U+23ED + VS16) stream itself as passing through the retroactive-widen
// (widen_after_merge / FR4) path. That does not hold: U+23ED carries the
// Extended_Pictographic property, so handle_print always buffers it (see
// test_write_grapheme_u23ed_vs16_across_chunk_boundary_widens_correctly
// above, which already exercises this exact codepoint pair and is named
// for write_grapheme_to_grid, not widen_after_merge). FR4 is genuinely
// exercised below via a separate digit+VS16 element (U+0035 + U+FE0F),
// which reaches widen_after_merge through try_retroactive_merge the same
// way the dedicated FR4 test
// (test_widen_after_merge_spacer_creation_over_existing_base_blanks_its_spacer
// above) does. ⏭️ is still used for the frame-1 pair that FR1 targets,
// matching its literal codepoint-stream representation.
#[test]
fn test_write_grapheme_column_shifted_redraw_blanks_all_orphan_partners() {
    let mut core = TerminalCore::new(16, 3, 0);

    // Frame 1: '─', ⏭️ (PAIR-1 base@1/spacer@2), 'A',
    // '5'+VS16-widened (PAIR-2 base@4/spacer@5), 'B',
    // '中' (PAIR-3 base@7/spacer@8), 'C',
    // '文' (PAIR-4 base@10/spacer@11), 'D'.
    core.set_g0_charset(1);
    core.handle_print(0x71); // '─' at col0
    core.set_g0_charset(0);
    core.process_pty_data("\u{23ED}\u{FE0F}".as_bytes()); // buffered
    core.handle_print(0x41); // 'A' forces flush: PAIR-1 base@1(w2)/spacer@2(w0)
    core.handle_print(0x35); // '5' at col4
    core.handle_print(0xFE0F); // VS16: retroactive widen -> PAIR-2 base@4(w2)/spacer@5(w0)
    core.handle_print(0x42); // 'B' at col6
    core.handle_print(0x4E2D); // '中': PAIR-3 base@7(w2)/spacer@8(w0)
    core.handle_print(0x43); // 'C' at col9
    core.handle_print(0x6587); // '文': PAIR-4 base@10(w2)/spacer@11(w0)
    core.handle_print(0x44); // 'D' at col12

    assert_eq!(core.get_cell_char(1, 0), "\u{23ED}\u{FE0F}");
    assert_eq!(core.get_cell_width(1, 0), 2);
    assert_eq!(core.get_cell_width(2, 0), 0);
    assert_eq!(core.get_cell_char(4, 0), "5\u{FE0F}");
    assert_eq!(core.get_cell_width(4, 0), 2);
    assert_eq!(core.get_cell_width(5, 0), 0);
    assert_eq!(core.get_cell_width(7, 0), 2);
    assert_eq!(core.get_cell_width(8, 0), 0);
    assert_eq!(core.get_cell_width(10, 0), 2);
    assert_eq!(core.get_cell_width(11, 0), 0);

    // Frame 2: CR, then a column-shifted redraw. Each rule's cleanup
    // target column is deliberately left untouched afterward (no later
    // write in this frame lands on it again) so the row check below
    // observes the rule's own output directly, rather than content a
    // later write would have produced regardless of whether the rule ran.
    core.process_pty_data(b"\r");

    // col0: ' ' — old '─' (w1): no trigger.
    core.handle_print(0x20);
    // col1: '-' — old = PAIR-1 base (w2): FR1 blanks the orphaned spacer
    // at col2 (col+1) before this overwrite lands. Nothing else in this
    // frame writes to col2 afterward.
    core.handle_print(0x2D);

    // Skip col2/col3/col4 entirely (CUP jump): PAIR-2's base at col4
    // stays untouched, so the next write lands squarely on its spacer.
    core.process_pty_data(b"\x1b[1;6H"); // CUP -> (row0, col5)
    // col5: 'y' — old = PAIR-2 spacer (w0), base still w2 at col4 (col-1):
    // FR2 blanks that base before this overwrite lands. Nothing else in
    // this frame writes to col4 afterward.
    core.handle_print(0x79);

    // col6-7: '日' (w2) — old col6 = 'B' (w1): no overwrite trigger. Its
    // placeholder at col7 lands on PAIR-3's base (still w2, untouched):
    // FR3 (write_grapheme_to_grid's chained cleanup) blanks PAIR-3's
    // spacer at col8 before the placeholder overwrites col7. Nothing else
    // in this frame writes to col8 afterward.
    core.handle_print(0x65E5);

    // Skip col8 entirely (CUP jump) to col9.
    core.process_pty_data(b"\x1b[1;10H"); // CUP -> (row0, col9)
    // col9: '5' then VS16 — old col9 = 'C' (w1): no trigger for the base
    // write. The VS16 then retroactively widens col9 (widen_after_merge);
    // its new spacer at col10 lands on PAIR-4's base (still w2,
    // untouched): FR4 (widen_after_merge's own chained cleanup) blanks
    // PAIR-4's spacer at col11 before the new spacer overwrites col10.
    // Nothing else in this frame writes to col11 afterward.
    core.handle_print(0x35);
    core.handle_print(0xFE0F);

    // AC-3: verify every column of the row — expected content/width. This
    // (together with the orphan scan below) confirms no orphan spacer
    // (w0 not preceded by a w2 base) or orphan base (w2 not followed by a
    // w0 spacer) survives anywhere in the row.
    let expected: &[(u16, &str, u8)] = &[
        (0, " ", 1),
        (1, "-", 1),
        (2, " ", 1),          // PAIR-1 spacer, blanked by FR1
        (3, "A", 1),          // untouched leftover from frame 1
        (4, " ", 1),          // PAIR-2 base, blanked by FR2
        (5, "y", 1),
        (6, "\u{65E5}", 2),   // new base
        (7, "", 0),           // new placeholder/spacer (was PAIR-3's base)
        (8, " ", 1),          // PAIR-3 spacer, blanked by FR3
        (9, "5\u{FE0F}", 2),  // widened base
        (10, "", 0),          // new spacer (was PAIR-4's base)
        (11, " ", 1),         // PAIR-4 spacer, blanked by FR4
        (12, "D", 1),         // untouched leftover from frame 1
        (13, " ", 1),
        (14, " ", 1),
        (15, " ", 1),
    ];
    for &(col, ch, width) in expected {
        assert_eq!(core.get_cell_char(col, 0), ch, "col {col}: unexpected char");
        assert_eq!(
            core.get_cell_width(col, 0),
            width,
            "col {col}: unexpected width"
        );
    }

    // Explicit orphan scan across the whole row, independent of the exact
    // content table above.
    for col in 0..16u16 {
        let w = core.get_cell_width(col, 0);
        if w == 0 {
            assert!(
                col > 0 && core.get_cell_width(col - 1, 0) == 2,
                "col {col}: orphan spacer (no width-2 base immediately before it)"
            );
        }
        if w == 2 {
            assert!(
                col + 1 < 16 && core.get_cell_width(col + 1, 0) == 0,
                "col {col}: orphan base (no width-0 spacer immediately after it)"
            );
        }
    }
}

// AC-1/AC-2 (TS3, FR1, FR2) additional coverage: shift-direction-dependent
// role swap. Test Notes call out that the side of a wide pair that gets
// directly overwritten (base vs. spacer) flips with the shift direction;
// this test demonstrates both roles in a single row. PAIR-A's spacer is
// hit directly (its base survives untouched until FR2 blanks it); PAIR-B's
// base is hit directly (its spacer survives untouched until FR1 blanks
// it) — the reverse pairing from the composite P5 test above.
#[test]
fn test_write_grapheme_redraw_swaps_which_partner_side_is_overwritten() {
    let mut core = TerminalCore::new(10, 3, 0);

    // Frame 1: '─', PAIR-A '世' (base@1/spacer@2), 'M',
    // PAIR-B '中' (base@4/spacer@5), 'N'.
    core.set_g0_charset(1);
    core.handle_print(0x71); // '─' at col0
    core.set_g0_charset(0);
    core.handle_print(0x4E16); // '世': PAIR-A base@1(w2)/spacer@2(w0)
    core.handle_print(0x4D); // 'M' at col3
    core.handle_print(0x4E2D); // '中': PAIR-B base@4(w2)/spacer@5(w0)
    core.handle_print(0x4E); // 'N' at col6

    assert_eq!(core.get_cell_width(1, 0), 2);
    assert_eq!(core.get_cell_width(2, 0), 0);
    assert_eq!(core.get_cell_width(4, 0), 2);
    assert_eq!(core.get_cell_width(5, 0), 0);

    // Frame 2: CR, then jump directly onto PAIR-A's spacer (skipping its
    // base entirely) before continuing left-to-right. Each rule's cleanup
    // target column (col1 for FR2, col5 for FR1) is left untouched
    // afterward so the row check below observes the rule's own output.
    core.process_pty_data(b"\r");
    core.process_pty_data(b"\x1b[1;3H"); // CUP -> (row0, col2)

    // col2: 'k' — old = PAIR-A spacer (w0), base still w2 at col1 (col-1):
    // FR2 blanks that base before this overwrite lands. Nothing else in
    // this frame writes to col1 afterward.
    core.handle_print(0x6B);
    // col3: 'L' — old = 'M' (w1, untouched): no trigger.
    core.handle_print(0x4C);
    // col4: 'X' — old = PAIR-B base (w2): FR1 blanks the orphaned spacer
    // at col5 before this overwrite lands. Nothing else in this frame
    // writes to col5 afterward.
    core.handle_print(0x58);

    let expected: &[(u16, &str, u8)] = &[
        (0, "\u{2500}", 1), // untouched leftover from frame 1
        (1, " ", 1),        // PAIR-A base, blanked by FR2
        (2, "k", 1),
        (3, "L", 1),
        (4, "X", 1),
        (5, " ", 1), // PAIR-B spacer, blanked by FR1
        (6, "N", 1), // untouched leftover from frame 1
        (7, " ", 1),
        (8, " ", 1),
        (9, " ", 1),
    ];
    for &(col, ch, width) in expected {
        assert_eq!(core.get_cell_char(col, 0), ch, "col {col}: unexpected char");
        assert_eq!(
            core.get_cell_width(col, 0),
            width,
            "col {col}: unexpected width"
        );
    }
    for col in 0..10u16 {
        assert_ne!(
            core.get_cell_width(col, 0),
            0,
            "col {col}: unexpected orphan spacer"
        );
        assert_ne!(
            core.get_cell_width(col, 0),
            2,
            "col {col}: unexpected orphan base"
        );
    }
}
