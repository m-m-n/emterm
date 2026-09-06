use crate::cell::{PackedColor, STYLE_BOLD, STYLE_UNDERLINE};
use crate::ring_buffer::ScrollDirection;
use crate::terminal_core::TerminalCore;

// ── Ring buffer index mapping tests ──────────────────

#[test]
fn test_viewport_abs_no_scrollback() {
    // With scrollback_lines=0: ring_capacity=rows, ring_head=0
    // viewport_abs(r) = r
    let core = TerminalCore::new(80, 24, 0);
    for r in 0..24u16 {
        assert_eq!(core.viewport_abs(r), r as usize);
    }
}

#[test]
fn test_viewport_abs_with_scrollback_capacity() {
    // scrollback_lines=100 — viewport ring is still sized for 24 rows.
    let core = TerminalCore::new(80, 24, 100);
    for r in 0..24u16 {
        assert_eq!(core.viewport_abs(r), r as usize);
    }
}

#[test]
fn test_scrollback_count_initial_no_scrollback() {
    let core = TerminalCore::new(80, 24, 0);
    assert_eq!(core.scrollback_count(), 0);
}

#[test]
fn test_viewport_cell_offset_basic() {
    let core = TerminalCore::new(80, 24, 0);
    // With no scrollback: offset = row * cols + col (same as flat grid)
    assert_eq!(core.viewport_cell_offset(0, 0), Some(0));
    assert_eq!(core.viewport_cell_offset(5, 0), Some(5));
    assert_eq!(core.viewport_cell_offset(0, 1), Some(80));
    assert_eq!(core.viewport_cell_offset(79, 23), Some(23 * 80 + 79));
}

#[test]
fn test_viewport_cell_offset_oob() {
    let core = TerminalCore::new(80, 24, 0);
    assert_eq!(core.viewport_cell_offset(80, 0), None);
    assert_eq!(core.viewport_cell_offset(0, 24), None);
    assert_eq!(core.viewport_cell_offset(80, 24), None);
}

#[test]
fn test_scrollback_count_initial() {
    let core = TerminalCore::new(80, 24, 100);
    assert_eq!(core.scrollback_count(), 0);
}

#[test]
fn test_scrollback_count_no_capacity() {
    let core = TerminalCore::new(80, 24, 0);
    assert_eq!(core.scrollback_count(), 0);
}

#[test]
fn test_ring_cell_offset() {
    let core = TerminalCore::new(80, 24, 0);
    assert_eq!(core.ring_cell_offset(0, 0), 0);
    assert_eq!(core.ring_cell_offset(0, 5), 5);
    assert_eq!(core.ring_cell_offset(3, 0), 3 * 80);
}

#[test]
fn test_constructor_with_scrollback() {
    let core = TerminalCore::new(80, 24, 1000);
    assert_eq!(core.cols(), 80);
    assert_eq!(core.rows(), 24);
    assert_eq!(core.ring_capacity, 1024); // 1000 + 24
    assert_eq!(core.ring_size, 24);
    assert_eq!(core.ring_head, 0);
    assert_eq!(core.scrollback_capacity, 1000);
}

#[test]
fn test_constructor_zero_scrollback_matches_flat() {
    let core = TerminalCore::new(10, 5, 0);
    assert_eq!(core.ring_capacity, 5);
    assert_eq!(core.ring_size, 5);
    assert_eq!(core.ring_head, 0);
    assert_eq!(core.scrollback_capacity, 0);
    // All cells should be empty
    for r in 0..5 {
        assert!(core.is_line_empty(r));
    }
}

// ── Ring push / scroll internal tests ─────────────────

#[test]
fn test_ring_push_blank_grows_scrollback() {
    let mut core = TerminalCore::new(10, 3, 5);
    assert_eq!(core.scrollback_count(), 0);
    core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.ring_push_blank(PackedColor::DEFAULT);
    assert_eq!(core.scrollback_count(), 1);
    // Old row 0 ("A") is now in scrollback, viewport row 0 is old row 1
    assert_eq!(core.get_cell_char(0, 0), " "); // old row 1 was empty
}

#[test]
fn test_ring_push_blank_at_capacity_evicts() {
    let mut core = TerminalCore::new(10, 3, 2);
    // scrollback capacity = 2.
    core.ring_push_blank(PackedColor::DEFAULT); // scrollback = 1
    core.ring_push_blank(PackedColor::DEFAULT); // scrollback = 2 (at capacity)
    assert_eq!(core.scrollback_count(), 2);
    // Next push should evict oldest
    core.ring_push_blank(PackedColor::DEFAULT);
    assert_eq!(core.scrollback_count(), 2); // still 2 (oldest evicted, newest added)
}

#[test]
fn test_scroll_up_internal_full_screen() {
    let mut core = TerminalCore::new(10, 3, 5);
    // Fill viewport
    for r in 0..3 {
        core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.scroll_up_internal(1);
    // Row 0 should now have old row 1
    assert_eq!(core.get_cell_char(0, 0), "1");
    assert_eq!(core.get_cell_char(0, 1), "2");
    assert_eq!(core.get_cell_char(0, 2), " "); // new blank line
    assert_eq!(core.scrollback_count(), 1);
}

#[test]
fn test_scroll_up_internal_region() {
    let mut core = TerminalCore::new(10, 5, 5);
    for r in 0..5 {
        core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.set_scroll_region(1, 3);
    core.scroll_up_internal(1);
    // Row 0 unchanged
    assert_eq!(core.get_cell_char(0, 0), "0");
    // Region shifted up: row 1 = old row 2, row 2 = old row 3, row 3 = blank
    assert_eq!(core.get_cell_char(0, 1), "2");
    assert_eq!(core.get_cell_char(0, 2), "3");
    assert_eq!(core.get_cell_char(0, 3), " ");
    // Row 4 unchanged
    assert_eq!(core.get_cell_char(0, 4), "4");
    // No scrollback growth (region scroll)
    assert_eq!(core.scrollback_count(), 0);
}

#[test]
fn test_scroll_down_internal() {
    let mut core = TerminalCore::new(10, 5, 5);
    for r in 0..5 {
        core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.scroll_down_internal(1);
    assert_eq!(core.get_cell_char(0, 0), " "); // new blank
    assert_eq!(core.get_cell_char(0, 1), "0");
    assert_eq!(core.get_cell_char(0, 2), "1");
    assert_eq!(core.scrollback_count(), 0); // no scrollback for scroll down
}

// ── Scrollback access API tests ─────────────────────

#[test]
fn test_get_scrollback_length_initial() {
    let core = TerminalCore::new(10, 3, 5);
    assert_eq!(core.get_scrollback_length(), 0);
}

#[test]
fn test_get_scrollback_length_after_scroll() {
    let mut core = TerminalCore::new(10, 3, 5);
    for r in 0..3 {
        core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.scroll_up_internal(1);
    assert_eq!(core.get_scrollback_length(), 1);
    core.scroll_up_internal(2);
    assert_eq!(core.get_scrollback_length(), 3);
}

#[test]
fn test_get_scrollback_length_capped_at_capacity() {
    let mut core = TerminalCore::new(10, 3, 2);
    for _ in 0..10 {
        core.scroll_up_internal(1);
    }
    assert_eq!(core.get_scrollback_length(), 2);
}

#[test]
fn test_get_scrollback_text_basic() {
    let mut core = TerminalCore::new(10, 3, 5);
    // Fill viewport row 0 with "Hello"
    for (i, ch) in "Hello".chars().enumerate() {
        core.set_cell(i as u16, 0, &ch.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    // Scroll so row 0 goes to scrollback
    core.scroll_up_internal(1);
    assert_eq!(core.get_scrollback_length(), 1);
    assert_eq!(core.get_scrollback_text(0), "Hello");
}

#[test]
fn test_get_scrollback_text_trims_trailing() {
    let mut core = TerminalCore::new(10, 3, 5);
    core.set_cell(0, 0, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    // Rest of row is spaces
    core.scroll_up_internal(1);
    assert_eq!(core.get_scrollback_text(0), "X");
}

#[test]
fn test_get_scrollback_text_oob_returns_empty() {
    let core = TerminalCore::new(10, 3, 5);
    assert_eq!(core.get_scrollback_text(0), "");
    assert_eq!(core.get_scrollback_text(999), "");
}

#[test]
fn test_get_scrollback_row_packed_matches_viewport() {
    let mut core = TerminalCore::new(10, 3, 5);
    // Fill row 0 with "A" cells
    for c in 0..10 {
        core.set_cell_ascii(c, 0, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    // Get packed before scrolling (viewport row 0)
    let before = core.get_row_packed(0);
    // Scroll so row 0 goes to scrollback
    core.scroll_up_internal(1);
    // Get scrollback row 0 packed
    let after = core.get_scrollback_row_packed(0);
    // Packed format should be identical
    assert_eq!(before, after);
}

#[test]
fn test_get_scrollback_row_packed_oob_returns_empty() {
    let core = TerminalCore::new(10, 3, 5);
    assert!(core.get_scrollback_row_packed(0).is_empty());
    assert!(core.get_scrollback_row_packed(999).is_empty());
}

#[test]
fn test_get_scrollback_row_cells_basic() {
    let mut core = TerminalCore::new(5, 3, 5);
    core.set_cell(0, 0, "H", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(1, 0, "i", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    let cells = core.get_scrollback_row_cells(0);
    // 5 cols: "H", "i", then 3 blank cells. Blank cells decode to a
    // single space (matching the viewport `get_cell_char`, which
    // returns " " for an empty cell), each width 1.
    assert_eq!(cells.len(), 5);
    assert_eq!(cells[0], ("H".to_string(), 1));
    assert_eq!(cells[1], ("i".to_string(), 1));
    assert_eq!(cells[2], (" ".to_string(), 1));
}

#[test]
fn test_get_scrollback_row_cells_wide_char() {
    let mut core = TerminalCore::new(5, 3, 5);
    // A double-width CJK glyph occupies col 0 (width 2); col 1 is its
    // width-0 continuation half and must be dropped from the result.
    core.set_cell(0, 0, "あ", 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(1, 0, "", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    let cells = core.get_scrollback_row_cells(0);
    assert_eq!(cells[0], ("あ".to_string(), 2));
    // The continuation half is dropped, so the next kept cell is the
    // blank at col 2 (a space, width 1), not the width-0 cell at col 1.
    assert_eq!(cells[1], (" ".to_string(), 1));
}

#[test]
fn test_get_scrollback_row_cells_overflow_grapheme() {
    let mut core = TerminalCore::new(3, 3, 5);
    // A multi-codepoint grapheme (emoji ZWJ family) routes through the
    // CharTable / overflow path rather than the inline ascii path.
    let family = "👨‍👩‍👧";
    core.set_cell(0, 0, family, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(1, 0, "", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    let cells = core.get_scrollback_row_cells(0);
    assert_eq!(cells[0], (family.to_string(), 2));
}

#[test]
fn test_get_scrollback_row_cells_oob_returns_empty() {
    let core = TerminalCore::new(10, 3, 5);
    assert!(core.get_scrollback_row_cells(0).is_empty());
    assert!(core.get_scrollback_row_cells(999).is_empty());
}

#[test]
fn test_get_scrollback_row_cells_styled_basic() {
    let mut core = TerminalCore::new(5, 3, 5);
    // "H" with RGB fg (200,100,50) + bold; "i" with default style.
    // set_cell args: col, row, char, width, fg_tag, fg_r, fg_g, fg_b,
    //                bg_tag, bg_r, bg_g, bg_b, flags
    core.set_cell(0, 0, "H", 1, 2, 200, 100, 50, 0, 0, 0, 0, STYLE_BOLD);
    core.set_cell(1, 0, "i", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    let cells = core.get_scrollback_row_cells_styled(0);
    // 5 cols: "H", "i", then 3 blank cells.
    assert_eq!(cells.len(), 5);
    assert_eq!(cells[0].glyph, "H");
    assert_eq!(cells[0].width, 1);
    // tag=2 (RGB) packed: tag<<24 | r<<16 | g<<8 | b.
    assert_eq!(cells[0].fg, (2u32 << 24) | (200 << 16) | (100 << 8) | 50);
    assert_eq!(cells[0].flags & STYLE_BOLD, STYLE_BOLD);
    // "i" carries the default style.
    assert_eq!(cells[1].glyph, "i");
    assert_eq!(cells[1].fg, 0);
    assert_eq!(cells[1].bg, 0);
    assert_eq!(cells[1].flags, 0);
    // Blank cell decodes to a space with default style.
    assert_eq!(cells[2].glyph, " ");
    assert_eq!(cells[2].width, 1);
}

#[test]
fn test_get_scrollback_row_cells_styled_packed_matches_viewport() {
    let mut core = TerminalCore::new(5, 3, 5);
    // Indexed fg (idx 3) + indexed bg (idx 5) + underline.
    core.set_cell(0, 0, "X", 1, 1, 3, 0, 0, 1, 5, 0, 0, STYLE_UNDERLINE);
    // Read the viewport-packed style before scrolling.
    let vp_fg = core.get_cell_fg(0, 0);
    let vp_bg = core.get_cell_bg(0, 0);
    let vp_flags = core.get_cell_flags(0, 0);
    core.scroll_up_internal(1);
    let cells = core.get_scrollback_row_cells_styled(0);
    // The styled scrollback cell must pack identically to the viewport
    // accessors so a renderer can reuse the same style resolution.
    assert_eq!(cells[0].fg, vp_fg);
    assert_eq!(cells[0].bg, vp_bg);
    assert_eq!(cells[0].flags, vp_flags);
}

#[test]
fn test_get_scrollback_row_cells_styled_wide_char() {
    let mut core = TerminalCore::new(5, 3, 5);
    // Double-width glyph at col 0; col 1 is its width-0 continuation and
    // must be dropped from the styled result.
    core.set_cell(0, 0, "あ", 2, 2, 10, 20, 30, 0, 0, 0, 0, 0);
    core.set_cell(1, 0, "", 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    let cells = core.get_scrollback_row_cells_styled(0);
    assert_eq!(cells[0].glyph, "あ");
    assert_eq!(cells[0].width, 2);
    assert_eq!(cells[0].fg, (2u32 << 24) | (10 << 16) | (20 << 8) | 30);
    // Next kept cell is the blank at col 2, not the dropped width-0 half.
    assert_eq!(cells[1].glyph, " ");
    assert_eq!(cells[1].width, 1);
}

#[test]
fn test_get_scrollback_row_cells_styled_oob_returns_empty() {
    let core = TerminalCore::new(10, 3, 5);
    assert!(core.get_scrollback_row_cells_styled(0).is_empty());
    assert!(core.get_scrollback_row_cells_styled(999).is_empty());
}

#[test]
fn test_scrollback_ordering_oldest_first() {
    let mut core = TerminalCore::new(10, 3, 5);
    // Fill each viewport row with a different letter
    core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(0, 1, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.set_cell(0, 2, "C", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    // Scroll up 2 times: A and B go to scrollback
    core.scroll_up_internal(2);
    assert_eq!(core.get_scrollback_length(), 2);
    // index 0 = oldest = "A"
    assert_eq!(core.get_scrollback_text(0), "A");
    // index 1 = newer = "B"
    assert_eq!(core.get_scrollback_text(1), "B");
}

#[test]
fn test_scrollback_eviction_oldest() {
    let mut core = TerminalCore::new(10, 3, 2);
    core.set_cell(0, 0, "A", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1); // A in scrollback[0]
    core.set_cell(0, 0, "B", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1); // A=scrollback[0], B=scrollback[1]
    assert_eq!(core.get_scrollback_text(0), "A");
    assert_eq!(core.get_scrollback_text(1), "B");
    // One more scroll: A evicted, B becomes oldest
    core.set_cell(0, 0, "C", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    assert_eq!(core.get_scrollback_length(), 2);
    assert_eq!(core.get_scrollback_text(0), "B");
    assert_eq!(core.get_scrollback_text(1), "C");
}

#[test]
fn test_scroll_up_internal_full_screen_no_scrollback_capacity() {
    let mut core = TerminalCore::new(10, 3, 0);
    for r in 0..3 {
        core.set_cell(0, r, &format!("{r}"), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    core.scroll_up_internal(1);
    // Should scroll (evict immediately since at capacity)
    assert_eq!(core.get_cell_char(0, 0), "1");
    assert_eq!(core.get_cell_char(0, 1), "2");
    assert_eq!(core.get_cell_char(0, 2), " ");
    assert_eq!(core.scrollback_count(), 0); // no room for scrollback
}

// Proved: `ring_push_blank`'s scrollback-enabled compress branch clears the
// `overflow` / `overflow_ridx` entries of the row it evicts and does not
// sweep away the entries of any other row — the survivor row's entries
// remain intact across the same push. Not proved: that the compress
// branch's own row-scoped clear call is what fired, as opposed to the
// unconditional new-viewport-bottom clear that runs after every push.
// Within a single push the new bottom absolute row always equals the
// evicted absolute row, independent of the row count, so the eviction-time
// clear and the unconditional bottom-row clear necessarily target the same
// absolute row and no fixture can distinguish them (mirrors the same
// structural ceiling documented on
// `test_ring_push_blank_clears_recycled_row_overflow_entries`, which
// exercises the scrollback-disabled eviction branch instead).
#[test]
fn test_ring_push_blank_clears_ridx() {
    let mut core = TerminalCore::new(10, 3, 2); // 10 cols, 3 rows, 2 scrollback lines

    // Overflow-bound content: multi-codepoint ZWJ grapheme clusters that
    // exceed the 16-byte inline cell capacity, so they are genuinely stored
    // in the `overflow` / `overflow_ridx` side tables rather than inline.
    let recycled_content = "👨‍👩‍👧‍👦"; // 4-person family
    let survivor_content = "👨‍👩‍👧"; // 3-person family

    // Recycled row: viewport row 0, the row the first push evicts.
    core.set_cell(0, 0, recycled_content, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    // Survivor row: viewport row 1, not evicted by the first push.
    core.set_cell(1, 1, survivor_content, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

    // Absolute row numbers, captured before the push rotates the ring head.
    // Viewport-relative indices do not denote the same absolute row after a
    // push, so every later assertion below reuses these verbatim.
    let abs_recycled = core.viewport_abs(0) as u32;
    let abs_survivor = core.viewport_abs(1) as u32;

    // Pre-assert non-vacuity (anti-vacuity guard): both rows genuinely hold
    // overflow-bound content before the push, independently through both
    // tables, so a fixture that fails to exceed the inline cap cannot
    // silently make the post-push "entry is gone" assertions pass for the
    // wrong reason.
    assert!(core.overflow.contains_key(&(0u32, abs_recycled)));
    assert!(
        core.overflow_ridx
            .get(&abs_recycled)
            .map(|cols| cols.contains(&0u32))
            .unwrap_or(false)
    );
    assert!(core.overflow.contains_key(&(1u32, abs_survivor)));
    assert!(
        core.overflow_ridx
            .get(&abs_survivor)
            .map(|cols| cols.contains(&1u32))
            .unwrap_or(false)
    );

    // Push one blank row: the compress branch runs (scrollback capacity >
    // 0), evicting the recycled row into scrollback.
    core.ring_push_blank(PackedColor::DEFAULT);

    // Post-assert row scope, as four independent assertions: the recycled
    // row's entries are gone from both tables, while the survivor row's
    // entries remain in both. Never merge these into two combined
    // assertions — a regression that drops only one table while keeping the
    // other must still be caught.
    assert!(!core.overflow.contains_key(&(0u32, abs_recycled)));
    assert!(!core.overflow_ridx.contains_key(&abs_recycled));
    assert!(core.overflow.contains_key(&(1u32, abs_survivor)));
    assert!(
        core.overflow_ridx
            .get(&abs_survivor)
            .map(|cols| cols.contains(&1u32))
            .unwrap_or(false)
    );

    // Push four more blanks (five in total, the pre-existing count) to
    // evict the overflow rows out of viewport AND scrollback.
    for _ in 0..4 {
        core.ring_push_blank(PackedColor::DEFAULT);
    }
    // Overflow side-table should be drained (the data was moved into CharTable
    // when the row was compressed). After eviction from scrollback the CharTable
    // refcount drops back to zero.
    assert!(core.overflow.is_empty());
    assert!(core.overflow_ridx.is_empty());
}

// AC-1–AC-5 (FR1–FR6): `ring_push_blank`'s row-scoped clearing of the
// overflow table and reverse index, pinned directly on the evicted absolute
// row and nowhere else. The fixture carries two overflow-bound rows — the
// row the scroll recycles and a second, non-recycled survivor row — so the
// test can show the clear is scoped to the recycled row specifically, not
// merely that clearing happened at eviction time. Complements
// `test_ring_push_blank_clears_ridx` above (which exercises the
// scrollback-enabled compress path via `set_cell`); this fixture has zero
// scrollback capacity so `ring_push_blank` takes the scrollback-disabled
// eviction branch instead, and drives the pre-populated cells through
// `handle_print` (mirroring `print_handler/tests.rs`'s overflow fixture
// shape) rather than by direct cell mutation.
#[test]
fn test_ring_push_blank_clears_recycled_row_overflow_entries() {
    let mut core = TerminalCore::new(5, 2, 0); // 5 cols, 2 rows, no scrollback

    let marks: [u32; 8] = [
        0x0301, 0x0302, 0x0303, 0x0304, 0x0305, 0x0306, 0x0307, 0x0308,
    ];

    // Populate viewport row 0, col 0 with width-1 overflow-bound content.
    core.handle_print(0x65); // 'e'
    for &m in &marks {
        core.handle_print(m);
    }
    // Populate viewport row 0, col 1 with width-1 overflow-bound content.
    core.handle_print(0x66); // 'f'
    for &m in &marks {
        core.handle_print(m);
    }

    // Populate viewport row 1, col 0 with overflow-bound content, via the
    // same print path and marking technique as row 0's cells. Row 1 is the
    // survivor: the scroll below recycles row 0's ring slot, not row 1's,
    // so row 1's overflow entry must remain after the scroll even though
    // its viewport position shifts.
    core.set_cursor(0, 1);
    core.handle_print(0x67); // 'g'
    for &m in &marks {
        core.handle_print(m);
    }

    let abs0 = core.viewport_abs(0) as u32;
    let abs1 = core.viewport_abs(1) as u32; // survivor's key, captured before the scroll

    // Pre-assert (anti-vacuity guard): both the to-be-recycled row's and
    // the survivor row's entries are genuinely overflow-bound before the
    // scroll, so a fixture that fails to exceed the inline cap cannot
    // silently make either row's post-scroll assertions vacuous.
    assert!(core.overflow.contains_key(&(0u32, abs0)));
    assert!(core.overflow.contains_key(&(1u32, abs0)));
    assert!(
        core.overflow_ridx
            .get(&abs0)
            .map(|cols| cols.contains(&0u32) && cols.contains(&1u32))
            .unwrap_or(false)
    );
    assert!(core.overflow.contains_key(&(0u32, abs1)));
    assert!(
        core.overflow_ridx
            .get(&abs1)
            .map(|cols| cols.contains(&0u32))
            .unwrap_or(false)
    );

    // Move to the last row and emit a plain line feed: the full-screen
    // scroll path runs `ring_push_blank`, recycling the slot that was
    // holding row 0's data.
    core.set_cursor(0, 1);
    core.handle_execute(0x0A);

    // Post-assert, removal (preserved verbatim): neither column's entry
    // remains in the overflow table for the recycled row key, and the
    // reverse index no longer holds that row key at all.
    assert!(!core.overflow.contains_key(&(0u32, abs0)));
    assert!(!core.overflow.contains_key(&(1u32, abs0)));
    assert!(!core.overflow_ridx.contains_key(&abs0));

    // Post-assert, survival (new): the survivor row's overflow entry and
    // reverse-index membership must remain intact — this is what proves
    // the clear is scoped to the recycled row rather than a whole-table
    // clear that happens to coincide with it.
    //
    // NOTE: removing only ONE of the two clearing sites inside
    // `ring_push_blank` (the eviction-time clear or the new-bottom-row
    // clear) still leaves this test green. This holds unconditionally:
    // on every `ring_push_blank` call that pushes at least one row, the
    // new bottom absolute row equals the evicted absolute row,
    // independent of the row count and of the scrollback capacity, so
    // no fixture can pin the two sites independently. The reason is
    // evaluation order: `evicted_abs` is captured from `ring_head`
    // before `Step 1` runs; `Step 2` then rotates `ring_head` by one;
    // `Step 3` derives `new_bottom_abs` from the rotated `ring_head` —
    // modulo the row count, the two expressions name the same ring
    // slot. Step 3's overflow / overflow_ridx clear pair for that
    // slot is therefore always a no-op within a single push:
    // whichever eviction-time clear branch ran has already emptied
    // that same pair for that same absolute row. This "always a
    // no-op" claim covers only that clear pair — it says nothing
    // about the rest of Step 3. Step 3 also fills the new bottom
    // row's cells and resets its `ring_wrapped` flag, and neither
    // action has a counterpart on the eviction side: Step 1 never
    // fills cells, and it only ever reads `ring_wrapped`, never
    // resets it. Both the cell fill and the `ring_wrapped` reset are
    // therefore required, not redundant.
    assert!(core.overflow.contains_key(&(0u32, abs1)));
    assert!(
        core.overflow_ridx
            .get(&abs1)
            .map(|cols| cols.contains(&0u32))
            .unwrap_or(false)
    );

    // Post-assert, survival (new): the survivor row's ring slot key is still
    // the key backing viewport row 0 after the scroll (AC-1), and the
    // survivor row's own content — the base character plus its combining
    // marks, derived from the fixture's own mark sequence rather than
    // transcribed here — is still readable at that row (AC-2). A defect
    // confined to the blank-push routine's Step 3 fill target (corrupting
    // only the slice actually cleared, while the overflow-clear side still
    // targets the correct absolute row) leaves every assertion above green
    // but blanks the survivor row's content, which only the AC-2 observation
    // below can catch.
    assert_eq!(core.viewport_abs(0) as u32, abs1);

    let expected_survivor_grapheme: String = std::iter::once(char::from_u32(0x67).unwrap())
        .chain(marks.iter().map(|&m| char::from_u32(m).unwrap()))
        .collect();
    assert_eq!(core.get_cell_char(0, 0), expected_survivor_grapheme);
}

// ── Scroll event tests ──────────────────────────────────

#[test]
fn test_scroll_up_full_screen_count1_emits_scroll_event() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.clear_dirty();
    assert!(core.scroll_event.is_none());

    core.scroll_up_internal(1);

    let event = core.scroll_event.expect("scroll event should be Some");
    assert_eq!(event.direction, ScrollDirection::Up);
    assert_eq!(event.count, 1);

    assert!(!core.is_row_dirty(0));
    assert!(!core.is_row_dirty(12));
    assert!(!core.is_row_dirty(22));
    assert!(core.is_row_dirty(23));
}

#[test]
fn test_scroll_up_full_screen_count_gt1_no_scroll_event() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.clear_dirty();

    core.scroll_up_internal(3);

    assert!(core.scroll_event.is_none());
    assert!(core.is_row_dirty(0));
    assert!(core.is_row_dirty(12));
    assert!(core.is_row_dirty(23));
}

#[test]
fn test_scroll_up_scroll_region_no_scroll_event() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.scroll_region_top = 5;
    core.scroll_region_bottom = 20;
    core.clear_dirty();

    core.scroll_up_internal(1);

    assert!(core.scroll_event.is_none());
}

#[test]
fn test_scroll_event_cleared_correctly() {
    let mut core = TerminalCore::new(80, 24, 100);

    core.scroll_up_internal(1);
    let event = core.scroll_event.expect("scroll event should be Some");
    assert_eq!(event.direction, ScrollDirection::Up);
    assert_eq!(event.count, 1);
    assert_eq!(core.get_scroll_event_direction(), 1); // 1 = Up
    assert_eq!(core.get_scroll_event_count(), 1);

    core.clear_scroll_event();
    assert!(core.scroll_event.is_none());
    assert_eq!(core.get_scroll_event_direction(), 0);
    assert_eq!(core.get_scroll_event_count(), 0);
}

#[test]
fn test_scroll_up_count1_accumulates_scroll_events() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.clear_dirty();

    core.scroll_up_internal(1);
    core.scroll_up_internal(1);
    core.scroll_up_internal(1);

    let event = core.scroll_event.expect("scroll event should be Some");
    assert_eq!(event.direction, ScrollDirection::Up);
    assert_eq!(event.count, 3);

    assert!(core.is_row_dirty(23));
}

#[test]
fn test_scroll_up_count1_shifts_dirty_and_marks_last() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.clear_dirty();

    core.mark_row_dirty(15);
    core.mark_row_dirty(20);

    core.scroll_up_internal(1);

    assert!(!core.is_row_dirty(15), "row 15 should no longer be dirty");
    assert!(
        core.is_row_dirty(14),
        "row 14 should be dirty (shifted from 15)"
    );
    assert!(!core.is_row_dirty(20), "row 20 should no longer be dirty");
    assert!(
        core.is_row_dirty(19),
        "row 19 should be dirty (shifted from 20)"
    );
    assert!(core.is_row_dirty(23), "last row should be dirty");
}

#[test]
fn test_scroll_up_count1_shifts_row0_dirty_away() {
    let mut core = TerminalCore::new(80, 24, 100);
    core.clear_dirty();

    core.mark_row_dirty(0);
    core.mark_row_dirty(10);

    core.scroll_up_internal(1);

    assert!(
        !core.is_row_dirty(0),
        "row 0 should not be dirty (shifted away)"
    );
    assert!(
        core.is_row_dirty(9),
        "row 9 should be dirty (shifted from 10)"
    );
    assert!(core.is_row_dirty(23), "last row should be dirty");
}

#[test]
fn test_shift_dirty_down_by_one_across_word_boundary() {
    let mut core = TerminalCore::new(80, 128, 100); // 128 rows = 2 u64 words
    core.clear_dirty();

    core.mark_row_dirty(64);

    core.shift_dirty_down_by_one();

    assert!(
        core.is_row_dirty(63),
        "row 63 should be dirty (shifted from 64)"
    );
    assert!(
        !core.is_row_dirty(64),
        "row 64 should not be dirty (shifted to 63)"
    );
}

// ── BCE scroll tests ────────────────────────────────────

#[test]
fn test_bce_ring_push_blank() {
    let mut core = TerminalCore::new(10, 3, 5);
    let green = PackedColor::indexed(2);
    core.ring_push_blank(green);
    // The new blank line is now the last viewport row (row 2)
    for col in 0..10 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 2));
        assert_eq!(bg, green, "col {col}");
    }
}

#[test]
fn test_bce_scroll_up_full_screen() {
    let mut core = TerminalCore::new(10, 3, 5);
    for r in 0..3 {
        for c in 0..10 {
            core.set_cell_ascii(c, r, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
    }
    core.set_cursor_bg(1, 2, 0, 0); // green
    core.scroll_up_internal(1);
    // New bottom row (row 2) should have green bg
    for col in 0..10 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 2));
        assert_eq!(bg, PackedColor::indexed(2), "col {col}");
    }
}

#[test]
fn test_bce_scroll_down() {
    let mut core = TerminalCore::new(10, 3, 0);
    for r in 0..3 {
        for c in 0..10 {
            core.set_cell_ascii(c, r, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        }
    }
    core.set_cursor_bg(1, 2, 0, 0); // green
    core.scroll_down_internal(1);
    // New top row (row 0) should have green bg (via shift_rows_down BCE)
    for col in 0..10 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 0));
        assert_eq!(bg, PackedColor::indexed(2), "col {col}");
    }
}

#[test]
fn test_bce_ring_push_blank_default() {
    let mut core = TerminalCore::new(10, 3, 5);
    core.ring_push_blank(PackedColor::DEFAULT);
    for col in 0..10 {
        let bg = PackedColor::from_u32(core.get_cell_bg(col, 2));
        assert_eq!(bg, PackedColor::DEFAULT);
    }
}

// ── SlimCell-specific tests (Phase 2 NEW) ──────────────

#[test]
fn test_scrollback_dedup_same_style() {
    // 1 million cells with same style → StyleTable should hold 2 entries
    // (default + the one used).
    let mut core = TerminalCore::new(80, 1, 100);
    for _ in 0..50 {
        for c in 0..80 {
            core.set_cell(
                c, 0, "A", 1, 2, 100, 150, 200, // RGB fg
                0, 0, 0, 0, 0,
            );
        }
        core.scroll_up_internal(1);
    }
    // styles table should have exactly 2 entries: default + one custom
    assert_eq!(core.styles.live_entries(), 2);
}

#[test]
fn test_scrollback_zero_no_slim_cells() {
    // scrollback_lines = 0 → no scrollback ever.
    let mut core = TerminalCore::new(10, 3, 0);
    for r in 0..3 {
        core.set_cell(0, r, "X", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    }
    for _ in 0..10 {
        core.scroll_up_internal(1);
    }
    assert_eq!(core.scrollback_count(), 0);
    assert_eq!(core.styles.live_entries(), 1); // only default
}

#[test]
fn test_scrollback_overflow_zwj_round_trip() {
    // ZWJ family emoji in scrollback should survive via CharTable.
    let mut core = TerminalCore::new(10, 3, 5);
    let zwj = "👨‍👩‍👧‍👦";
    core.set_cell(0, 0, zwj, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    let text = core.get_scrollback_text(0);
    assert!(text.contains(zwj), "expected to find {zwj}, got '{text}'");
    // CharTable should have the entry
    assert_eq!(core.chars.live_entries(), 1);
}

#[test]
fn test_clear_scrollback_releases_refcounts() {
    let mut core = TerminalCore::new(10, 3, 5);
    let zwj = "👨‍👩‍👧‍👦";
    core.set_cell(0, 0, zwj, 2, 2, 100, 150, 200, 0, 0, 0, 0, 0);
    core.scroll_up_internal(1);
    assert_eq!(core.scrollback_count(), 1);
    assert_eq!(core.chars.live_entries(), 1);
    assert_eq!(core.styles.live_entries(), 2);

    core.clear_scrollback();
    assert_eq!(core.scrollback_count(), 0);
    // Tables should be back to baseline
    assert_eq!(core.chars.live_entries(), 0);
    assert_eq!(core.styles.live_entries(), 1);
}

// ── Bounded eviction (FR4) tests ────────────────────────

#[test]
fn test_evict_oldest_scrollback_to_target() {
    let mut core = TerminalCore::new(10, 3, 100);
    // Push 5 distinct rows into scrollback.
    for i in 0..5u32 {
        core.set_cell(0, 0, &i.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    assert_eq!(core.get_scrollback_length(), 5);
    // "0","1","2","3","4" oldest-first.
    assert_eq!(core.get_scrollback_text(0), "0");

    // Evict down to 2 lines: drops the 3 oldest ("0","1","2").
    let evicted = core.evict_oldest_scrollback(2);
    assert_eq!(evicted, 3);
    assert_eq!(core.get_scrollback_length(), 2);
    assert_eq!(core.get_scrollback_text(0), "3");
    assert_eq!(core.get_scrollback_text(1), "4");
}

#[test]
fn test_evict_oldest_scrollback_noop_when_below_target() {
    let mut core = TerminalCore::new(10, 3, 100);
    for i in 0..3u32 {
        core.set_cell(0, 0, &i.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    assert_eq!(core.get_scrollback_length(), 3);
    // Target above current → no eviction.
    let evicted = core.evict_oldest_scrollback(10);
    assert_eq!(evicted, 0);
    assert_eq!(core.get_scrollback_length(), 3);
    // Target equal to current → no eviction.
    let evicted = core.evict_oldest_scrollback(3);
    assert_eq!(evicted, 0);
    assert_eq!(core.get_scrollback_length(), 3);
}

#[test]
fn test_evict_oldest_scrollback_releases_refcounts() {
    let mut core = TerminalCore::new(10, 3, 100);
    // 5 rows each with a distinct style.
    for i in 0..5u32 {
        core.set_cell(0, 0, "A", 1, 2, i as u8, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    assert_eq!(core.get_scrollback_length(), 5);
    core.evict_oldest_scrollback(2);
    assert_eq!(core.get_scrollback_length(), 2);
    // Only the 2 surviving rows' styles (+ default) remain live.
    assert!(
        core.styles.live_entries() <= 3,
        "got {} live styles",
        core.styles.live_entries()
    );
}

#[test]
fn test_evict_oldest_scrollback_to_zero() {
    let mut core = TerminalCore::new(10, 3, 100);
    for i in 0..4u32 {
        core.set_cell(0, 0, &i.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    let evicted = core.evict_oldest_scrollback(0);
    assert_eq!(evicted, 4);
    assert_eq!(core.get_scrollback_length(), 0);
}

#[test]
fn test_evicted_total_counts_api_eviction() {
    let mut core = TerminalCore::new(10, 3, 100);
    for i in 0..5u32 {
        core.set_cell(0, 0, &i.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    assert_eq!(core.get_scrollback_evicted_total(), 0);
    // Evict 3 of the 5 rows via the explicit API.
    core.evict_oldest_scrollback(2);
    assert_eq!(core.get_scrollback_evicted_total(), 3);
    // A no-op eviction must not advance the counter.
    core.evict_oldest_scrollback(2);
    assert_eq!(core.get_scrollback_evicted_total(), 3);
}

#[test]
fn test_evicted_total_counts_automatic_eviction() {
    // capacity 2 scrollback rows → rows beyond 2 evict automatically.
    let mut core = TerminalCore::new(10, 3, 2);
    // Push 5 rows; the first 3 spill out of the 2-row scrollback.
    for i in 0..5u32 {
        core.set_cell(0, 0, &i.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    assert_eq!(core.get_scrollback_length(), 2);
    assert_eq!(core.get_scrollback_evicted_total(), 3);
}

#[test]
fn test_evicted_total_reset_zeroes_counter() {
    let mut core = TerminalCore::new(10, 3, 100);
    for i in 0..5u32 {
        core.set_cell(0, 0, &i.to_string(), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    core.evict_oldest_scrollback(2);
    assert_eq!(core.get_scrollback_evicted_total(), 3);
    core.reset();
    assert_eq!(core.get_scrollback_evicted_total(), 0);
}

#[test]
fn test_eviction_releases_refcounts() {
    let mut core = TerminalCore::new(10, 3, 2); // capacity 2 scrollback rows
    // Push 5 distinct rows; only the last 2 should remain.
    for i in 0..5u32 {
        // Use a unique style per row by varying RGB.
        core.set_cell(0, 0, "A", 1, 2, i as u8, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);
    }
    assert_eq!(core.scrollback_count(), 2);
    // StyleTable: default + (each cell uses 1 distinct style for col 0;
    // remaining 9 cols are blanks with default style). Live should be 2 + 1
    // = 3 (default + the 2 surviving styles for the kept rows). The other
    // 3 styles were evicted and their refcount went to 0.
    assert!(
        core.styles.live_entries() <= 3,
        "got {} live styles",
        core.styles.live_entries()
    );
}

// ── TS-14: Phase 1 snapshot-replay bypass branch in isolation ──────────

/// TS-14 (a): enable the bypass on a fresh core, scroll off N viewport
/// rows with N < scrollback_capacity. The virtual length must equal N,
/// the evicted-total counter must stay at 0, `get_scrollback_length()`
/// must mirror the virtual length, and `scrollback_count()` must remain
/// 0 (the SlimCell deque was intentionally not populated).
#[test]
fn test_bypass_branch_below_capacity_no_eviction() {
    let mut core = TerminalCore::new(10, 3, 5);
    core.enable_snapshot_bypass();
    // Push 2 viewport rows off the top (N=2, C=5 → N<C).
    for _ in 0..2 {
        core.ring_push_blank(PackedColor::DEFAULT);
    }
    assert_eq!(core.virtual_scrollback_len, 2);
    assert_eq!(core.get_scrollback_evicted_total(), 0);
    assert_eq!(core.get_scrollback_length(), 2);
    assert_eq!(core.scrollback_count(), 0);
    core.disable_snapshot_bypass();
}

/// TS-14 (b): enable the bypass on a fresh core, scroll off N viewport
/// rows with N > scrollback_capacity. The virtual length must saturate
/// at the capacity, the evicted-total counter must equal N - C, and
/// `scrollback_count()` must remain 0.
#[test]
fn test_bypass_branch_above_capacity_saturates_and_evicts() {
    let mut core = TerminalCore::new(10, 3, 5);
    core.enable_snapshot_bypass();
    // Push 8 viewport rows off the top (N=8, C=5 → N>C, expect 3 evicted).
    for _ in 0..8 {
        core.ring_push_blank(PackedColor::DEFAULT);
    }
    assert_eq!(core.virtual_scrollback_len, 5);
    assert_eq!(core.get_scrollback_evicted_total(), 3);
    assert_eq!(core.get_scrollback_length(), 5);
    assert_eq!(core.scrollback_count(), 0);
    core.disable_snapshot_bypass();
}

/// After `disable_snapshot_bypass`, `get_scrollback_length()` returns to
/// the live-mode `scrollback_count() as u32` value (= 0, because nothing
/// was retained), and `virtual_scrollback_len` is reset to 0.
#[test]
fn test_disable_bypass_resets_virtual_length_and_restores_live_branch() {
    let mut core = TerminalCore::new(10, 3, 5);
    core.enable_snapshot_bypass();
    for _ in 0..3 {
        core.ring_push_blank(PackedColor::DEFAULT);
    }
    assert_eq!(core.get_scrollback_length(), 3);
    core.disable_snapshot_bypass();
    // Live-mode branch returns scrollback_count() == 0.
    assert_eq!(core.virtual_scrollback_len, 0);
    assert_eq!(core.get_scrollback_length(), 0);
    // A subsequent live `ring_push_blank` populates the deque normally.
    core.ring_push_blank(PackedColor::DEFAULT);
    assert_eq!(core.scrollback_count(), 1);
    assert_eq!(core.get_scrollback_length(), 1);
}

/// `evicted_total` is not touched by `disable_snapshot_bypass`: its
/// monotonic semantics are part of the externally observable contract.
#[test]
fn test_disable_bypass_preserves_evicted_total() {
    let mut core = TerminalCore::new(10, 3, 2);
    core.enable_snapshot_bypass();
    for _ in 0..5 {
        core.ring_push_blank(PackedColor::DEFAULT);
    }
    assert_eq!(core.get_scrollback_evicted_total(), 3);
    core.disable_snapshot_bypass();
    assert_eq!(core.get_scrollback_evicted_total(), 3);
}

// ── EC-1 regression: scrollback_lines == 0 bypass must not bump counters ──

/// EC-1 / FR3: build_from_snapshot with scrollback_lines=0 must produce
/// the same evicted_total as the synchronous reset_and_replay path.
///
/// Before the fix, the bypass branch's inner counter logic was:
///   if virtual_scrollback_len < scrollback_capacity { virtual += 1 }
///   else { evicted_total += 1 }
/// When scrollback_capacity == 0, the condition `0 < 0` is always false,
/// so every viewport scroll-off hit the `else` arm and incorrectly
/// incremented `evicted_total`. The synchronous path takes the third
/// `else` arm (scrollback disabled, no counter bump), so the two paths
/// diverged. This test locks down the fix.
#[test]
fn test_build_from_snapshot_bypass_scrollback_zero_matches_sync_path() {
    // Build a payload that scrolls 100 lines into a 24-row grid.
    // Simple newlines are enough to trigger ring_push_blank via the
    // full-screen scroll path.
    let payload: Vec<u8> = b"\n".repeat(100);

    // Off-thread path: build_from_snapshot with scrollback_lines = 0.
    static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let replay = TerminalCore::build_from_snapshot(80, 24, 0, &payload, &[], &NEVER)
        .expect("build_from_snapshot must not be cancelled");
    let bypass_evicted = replay.evicted_total;

    // Synchronous path: fresh core + reset_and_replay.
    let mut sync_core = TerminalCore::new(80, 24, 0);
    sync_core.reset_and_replay(&payload);
    let sync_evicted = sync_core.get_scrollback_evicted_total();

    assert_eq!(
        bypass_evicted, sync_evicted,
        "EC-1: build_from_snapshot evicted_total ({bypass_evicted}) \
         must equal synchronous path ({sync_evicted}) when scrollback_lines==0"
    );
    // Both paths must report 0: scrollback is disabled, so no eviction
    // counter should ever be bumped regardless of how many lines scroll.
    assert_eq!(
        bypass_evicted, 0,
        "scrollback_lines==0 must never produce a non-zero evicted_total"
    );
}

// ── prepend_scrollback_rows (scrollback restore) ─────

/// Build a SlimCell row of `cols` "X" cells against the given tables.
/// Mirrors the cell_to_slim path used at eviction time.
fn make_slim_row_ascii_x(
    cols: usize,
    styles: &mut crate::style_table::StyleTable,
    chars: &mut crate::char_table::CharTable,
) -> Vec<crate::slim_cell::SlimCell> {
    let mut row = Vec::with_capacity(cols);
    for _ in 0..cols {
        let mut cell = crate::cell::Cell::EMPTY;
        cell.set_char("X");
        cell.width = 1;
        let slim = crate::slim_cell::cell_to_slim(&cell, None, styles, chars);
        row.push(slim);
    }
    row
}

/// TS-3 base case: prepend with enough room inserts every incoming row at
/// the **front** of the scrollback deque, oldest-first, leaving the
/// pre-existing rows untouched.
#[test]
fn test_prepend_scrollback_rows_fits_capacity_preserves_order() {
    // Existing core: viewport 4 wide × 2 tall, scrollback cap = 10.
    // Push two rows of live content so scrollback_count() == 2.
    let mut core = TerminalCore::new(4, 2, 10);
    core.process_pty_data_fully(b"AAAA\r\nBBBB\r\nCCCC\r\nDDDD\r\n");
    // CCCC / DDDD occupy the viewport; AAAA / BBBB sit in scrollback.
    let before_count = core.scrollback_count();
    assert!(
        before_count >= 2,
        "live scrollback should have at least 2 rows"
    );

    // Build two incoming rows interned against the core's own tables
    // (mirroring what merge_scrollback_from will do after re-intern).
    let mut incoming_rows: Vec<Vec<crate::slim_cell::SlimCell>> = Vec::new();
    for _ in 0..2 {
        incoming_rows.push(make_slim_row_ascii_x(4, &mut core.styles, &mut core.chars));
    }
    let incoming_wrapped = vec![false, true];
    let evicted_before = core.scrollback_evicted_total;

    let inserted = core.prepend_scrollback_rows(incoming_rows, incoming_wrapped);
    assert_eq!(inserted, 2);
    assert_eq!(core.scrollback_count(), before_count + 2);
    // The two front-most rows must be the just-prepended X rows.
    for col in 0..4 {
        assert_eq!(core.scrollback_slim[0][col].width, 1);
        assert_eq!(core.scrollback_slim[1][col].width, 1);
    }
    // wrapped flag preserved at the same front-most indices.
    assert!(!core.scrollback_wrapped[0]);
    assert!(core.scrollback_wrapped[1]);
    // Evicted total untouched (NFR5).
    assert_eq!(core.scrollback_evicted_total, evicted_before);
}

/// TS-3: capacity overflow drops the **front-most incoming** rows and
/// preserves the pre-existing rows.
#[test]
fn test_prepend_scrollback_rows_drops_front_most_incoming_on_overflow() {
    // Tight capacity = 5 to exercise the drop path.
    let mut core = TerminalCore::new(4, 2, 5);
    // 5 lines pushed → 3 in scrollback (viewport holds last 2). The
    // scrollback ring is below cap, leaving room = 2.
    core.process_pty_data_fully(b"AA\r\nBB\r\nCC\r\nDD\r\nEE\r\n");
    let before_count = core.scrollback_count();
    // Confirm precondition.
    assert!(before_count <= 5);

    // Capture the pre-existing front cell so we can prove it survives.
    let pre_existing_front: Vec<crate::slim_cell::SlimCell> =
        core.scrollback_slim.front().cloned().expect("non-empty");

    // Build 4 incoming rows but room is at most cap - before_count.
    let mut incoming_rows = Vec::new();
    for _ in 0..4 {
        incoming_rows.push(make_slim_row_ascii_x(4, &mut core.styles, &mut core.chars));
    }
    let incoming_wrapped = vec![false; 4];
    let evicted_before = core.scrollback_evicted_total;

    let inserted = core.prepend_scrollback_rows(incoming_rows, incoming_wrapped);
    let room = 5_usize.saturating_sub(before_count);
    assert_eq!(inserted, room, "must insert exactly the available room");
    assert_eq!(core.scrollback_count(), 5, "ring must be at capacity");
    // The (room)-th index from the front is the first pre-existing row.
    assert_eq!(
        core.scrollback_slim[room], pre_existing_front,
        "pre-existing front row must survive at index {room}"
    );
    // Evicted total untouched (NFR5).
    assert_eq!(core.scrollback_evicted_total, evicted_before);
}

/// scrollback_lines == 0 ⇒ prepend is a noop, every incoming row is
/// dec_ref'd but nothing is inserted.
#[test]
fn test_prepend_scrollback_rows_scrollback_disabled_drops_all() {
    let mut core = TerminalCore::new(4, 2, 0);
    let row = make_slim_row_ascii_x(4, &mut core.styles, &mut core.chars);
    let inserted = core.prepend_scrollback_rows(vec![row], vec![false]);
    assert_eq!(inserted, 0);
    assert_eq!(core.scrollback_count(), 0);
}
