use super::*;

// ── Grid construction ────────────────────────────────

#[test]
fn test_grid_new_80x24() {
    let core = TerminalCore::new(80, 24, 0);
    assert_eq!(core.cols(), 80);
    assert_eq!(core.rows(), 24);
    // All cells should be empty spaces
    for row in 0..24 {
        assert!(core.is_line_empty(row));
    }
}

// ── Cell set/get round-trip ──────────────────────────

#[test]
fn test_set_get_cell_ascii() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    assert_eq!(core.get_cell_char(0, 0), "A");
    assert_eq!(core.get_cell_width(0, 0), 1);
}

#[test]
fn test_set_get_cell_cjk() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.set_cell(5, 3, "漢", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    assert_eq!(core.get_cell_char(5, 3), "漢");
    assert_eq!(core.get_cell_width(5, 3), 2);
}

#[test]
fn test_set_get_cell_ascii_fast() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.set_cell_ascii(10, 5, b'Z', 2, 100, 200, 50, 0, 0, 0, 0, 0);
    assert_eq!(core.get_cell_char(10, 5), "Z");
    assert_eq!(core.get_cell_width(10, 5), 1);
    let fg = core.get_cell_fg(10, 5);
    assert_eq!(fg >> 24, 2); // tag = RGB
    assert_eq!((fg >> 16) & 0xFF, 100); // r
}

#[test]
fn test_set_get_cell_with_attrs() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Set with RGB fg, indexed bg, bold+italic
    core.set_cell(
        0,
        0,
        "X",
        1,
        2,
        255,
        128,
        64,
        1,
        42,
        0,
        0,
        STYLE_BOLD | STYLE_ITALIC,
    );
    assert_eq!(core.get_cell_char(0, 0), "X");
    let fg = core.get_cell_fg(0, 0);
    assert_eq!(PackedColor::from_u32(fg), PackedColor::rgb(255, 128, 64));
    let bg = core.get_cell_bg(0, 0);
    assert_eq!(PackedColor::from_u32(bg), PackedColor::indexed(42));
    assert_eq!(core.get_cell_flags(0, 0), STYLE_BOLD | STYLE_ITALIC);
}

// ── Out-of-bounds ────────────────────────────────────

#[test]
fn test_oob_write_noop() {
    let mut core = TerminalCore::new(80, 24, 0);
    // Should not panic
    core.set_cell(80, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(0, 24, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
}

#[test]
fn test_oob_read_default() {
    let core = TerminalCore::new(80, 24, 0);
    assert_eq!(core.get_cell_char(80, 0), " ");
    assert_eq!(core.get_cell_width(0, 24), 1);
    assert_eq!(core.get_cell_fg(100, 100), 0);
}

// ── Line operations ──────────────────────────────────

#[test]
fn test_clear_line() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(1, 0, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.clear_line(0);
    assert_eq!(core.get_cell_char(0, 0), " ");
    assert_eq!(core.get_cell_char(1, 0), " ");
    assert!(core.is_line_empty(0));
}

#[test]
fn test_clear_line_range() {
    let mut core = TerminalCore::new(80, 24, 0);
    for col in 0..10 {
        core.set_cell(col, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.clear_line_range(0, 3, 7);
    assert_eq!(core.get_cell_char(2, 0), "X");
    assert_eq!(core.get_cell_char(3, 0), " ");
    assert_eq!(core.get_cell_char(6, 0), " ");
    assert_eq!(core.get_cell_char(7, 0), "X");
}

#[test]
fn test_get_line_text() {
    let mut core = TerminalCore::new(10, 1, 0);
    core.set_cell(0, 0, "H", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(1, 0, "i", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    // Width-0 placeholder (e.g., second cell of wide char)
    let text = core.get_line_text(0);
    assert!(text.starts_with("Hi"));
}

#[test]
fn test_get_line_text_skips_width0() {
    let mut core = TerminalCore::new(10, 1, 0);
    core.set_cell(0, 0, "漢", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    // Set width=0 placeholder at col 1
    core.set_cell(1, 0, "", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let text = core.get_line_text(0);
    // Should have "漢" followed by spaces, not the empty placeholder
    assert!(text.starts_with("漢"));
    assert!(!text.contains('\0'));
}

#[test]
fn test_is_line_empty() {
    let mut core = TerminalCore::new(80, 24, 0);
    assert!(core.is_line_empty(0));
    core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    assert!(!core.is_line_empty(0));
}

// ── Row operations ───────────────────────────────────

#[test]
fn test_shift_rows_up() {
    let mut core = TerminalCore::new(10, 5, 0);
    // Set identifiable content on each row
    for row in 0..5 {
        core.set_cell(0, row, &format!("{row}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.shift_rows_up(0, 4, 2);
    // Row 0 should now have what was row 2
    assert_eq!(core.get_cell_char(0, 0), "2");
    assert_eq!(core.get_cell_char(0, 1), "3");
    assert_eq!(core.get_cell_char(0, 2), "4");
    // Bottom rows should be cleared
    assert_eq!(core.get_cell_char(0, 3), " ");
    assert_eq!(core.get_cell_char(0, 4), " ");
}

#[test]
fn test_shift_rows_down() {
    let mut core = TerminalCore::new(10, 5, 0);
    for row in 0..5 {
        core.set_cell(0, row, &format!("{row}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.shift_rows_down(0, 4, 2);
    // Top rows should be cleared
    assert_eq!(core.get_cell_char(0, 0), " ");
    assert_eq!(core.get_cell_char(0, 1), " ");
    // Original rows shifted down
    assert_eq!(core.get_cell_char(0, 2), "0");
    assert_eq!(core.get_cell_char(0, 3), "1");
    assert_eq!(core.get_cell_char(0, 4), "2");
}

#[test]
fn test_copy_row() {
    let mut core = TerminalCore::new(10, 5, 0);
    core.set_cell(0, 0, "X", 1, 2, 255, 0, 0, 0, 0, 0, 0, STYLE_BOLD);
    core.set_line_wrapped(0, true);
    core.copy_row(0, 3);
    assert_eq!(core.get_cell_char(0, 3), "X");
    assert_eq!(core.get_cell_flags(0, 3), STYLE_BOLD);
    assert!(core.get_line_wrapped(3));
}

#[test]
fn test_fill_row_default() {
    let mut core = TerminalCore::new(10, 5, 0);
    core.set_cell(0, 2, "Z", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.fill_row_default(2);
    assert!(core.is_line_empty(2));
}

// ── Resize ───────────────────────────────────────────

#[test]
fn test_resize_grow_cols() {
    let mut core = TerminalCore::new(10, 5, 0);
    core.set_cell(5, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.resize(20, 5);
    assert_eq!(core.cols(), 20);
    assert_eq!(core.get_cell_char(5, 0), "A");
}

#[test]
fn test_resize_shrink_cols() {
    let mut core = TerminalCore::new(10, 5, 0);
    core.set_cell(8, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.resize(5, 5);
    assert_eq!(core.cols(), 5);
    // Col 8 should be gone, reading it via get_cell_char returns default
    assert_eq!(core.get_cell_char(8, 0), " ");
}

#[test]
fn test_resize_grow_shrink_rows() {
    let mut core = TerminalCore::new(10, 5, 0);
    core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);

    // Grow
    core.resize(10, 10);
    assert_eq!(core.rows(), 10);
    assert_eq!(core.get_cell_char(0, 0), "A");

    // Shrink
    core.resize(10, 3);
    assert_eq!(core.rows(), 3);
    assert_eq!(core.get_cell_char(0, 0), "A");
}

// ── Reset ────────────────────────────────────────────

#[test]
fn test_reset() {
    let mut core = TerminalCore::new(80, 24, 0);
    core.set_cell(5, 5, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, STYLE_BOLD);
    core.set_cursor(40, 12);
    core.set_mode(MODE_BRACKETED_PASTE, true);
    core.reset();

    assert_eq!(core.get_cursor_col(), 0);
    assert_eq!(core.get_cursor_row(), 0);
    assert!(core.get_mode(MODE_AUTO_WRAP));
    assert!(!core.get_mode(MODE_BRACKETED_PASTE));
    assert!(core.is_line_empty(5));
}

/// AC-5: `reset()` fires the GUI-agnostic "full reset occurred" signal
/// (`TerminalCallbacks::on_reset`), the mechanism a host uses to restore
/// a theme-side OSC 12 cursor-color override (cursor-settings-fix FR4).
#[test]
fn test_reset_fires_on_reset_callback() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Recorder(Arc<AtomicUsize>);
    impl crate::callbacks::TerminalCallbacks for Recorder {
        fn on_osc(&self, _action_type: u8, _data: &str) {}
        fn on_apc(&self, _data: &[u8]) {}
        fn on_dcs(&self, _data: &[u8]) {}
        fn on_bell(&self) {}
        fn on_reset(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let counter = Arc::new(AtomicUsize::new(0));
    let mut core = TerminalCore::new(80, 24, 0);
    core.callbacks = Some(Box::new(Recorder(counter.clone())));
    core.reset();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

// ── Batch row packed ─────────────────────────────────

#[test]
fn test_get_row_packed_basic() {
    let mut core = TerminalCore::new(3, 1, 0);
    core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let packed = core.get_row_packed(0);
    assert!(!packed.is_empty());
    // First byte should be char_len=1, then 'A'
    assert_eq!(packed[0], 1); // char_len
    assert_eq!(packed[1], b'A'); // char data
}

// ── Overflow side table with shift ───────────────────

#[test]
fn test_overflow_remapped_on_shift_up() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    assert!(long.as_bytes().len() > 16);
    core.set_cell(0, 3, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    assert_eq!(core.get_cell_char(0, 3), long);

    core.shift_rows_up(0, 4, 2);
    // Row 3 shifted to row 1
    assert_eq!(core.get_cell_char(0, 1), long);
}

// ── Phase 4: Reverse index tests ────────────────────

#[test]
fn test_ridx_maintained_on_set_cell_overflow() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    core.set_cell(3, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let abs = core.viewport_abs(2) as u32;
    assert!(core.overflow_ridx.contains_key(&abs));
    assert!(core.overflow_ridx[&abs].contains(&3));
}

#[test]
fn test_ridx_removed_on_overwrite_with_ascii() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    core.set_cell(3, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let abs = core.viewport_abs(2) as u32;
    assert!(core.overflow_ridx.contains_key(&abs));

    // Overwrite with ASCII
    core.set_cell_ascii(3, 2, b'X', 0, 0, 0, 0, 0, 0, 0, 0, 0);
    assert!(!core.overflow_ridx.contains_key(&abs));
}

#[test]
fn test_ridx_maintained_after_shift_rows_up() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    core.set_cell(5, 3, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let old_abs = core.viewport_abs(3) as u32;
    assert!(core.overflow_ridx.contains_key(&old_abs));

    core.shift_rows_up(0, 4, 2);
    // Row 3 -> row 1
    let new_abs = core.viewport_abs(1) as u32;
    assert!(core.overflow_ridx.contains_key(&new_abs));
    assert!(core.overflow_ridx[&new_abs].contains(&5));
    // Old abs should be gone
    assert!(!core.overflow_ridx.contains_key(&old_abs));
}

#[test]
fn test_ridx_maintained_after_shift_rows_down() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    core.set_cell(5, 1, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let old_abs = core.viewport_abs(1) as u32;
    assert!(core.overflow_ridx.contains_key(&old_abs));

    core.shift_rows_down(0, 4, 2);
    // Row 1 -> row 3
    let new_abs = core.viewport_abs(3) as u32;
    assert!(core.overflow_ridx.contains_key(&new_abs));
    assert!(core.overflow_ridx[&new_abs].contains(&5));
}

#[test]
fn test_ridx_cleared_on_clear_line() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    core.set_cell(5, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let abs = core.viewport_abs(2) as u32;
    assert!(core.overflow_ridx.contains_key(&abs));

    core.clear_line(2);
    assert!(!core.overflow_ridx.contains_key(&abs));
}

#[test]
fn test_ridx_copy_row() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    core.set_cell(5, 1, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

    core.copy_row(1, 3);
    let dst_abs = core.viewport_abs(3) as u32;
    assert!(core.overflow_ridx.contains_key(&dst_abs));
    assert!(core.overflow_ridx[&dst_abs].contains(&5));
    // Source should still have it
    let src_abs = core.viewport_abs(1) as u32;
    assert!(core.overflow_ridx.contains_key(&src_abs));
}

#[test]
fn test_ridx_cleared_on_reset() {
    let mut core = TerminalCore::new(10, 5, 0);
    let long = "👨‍👩‍👧‍👦";
    core.set_cell(5, 2, long, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    assert!(!core.overflow_ridx.is_empty());

    core.reset();
    assert!(core.overflow_ridx.is_empty());
}
