/// Print handler: handle_print, flush_grapheme_buffer, grapheme buffer,
/// DEC charset translation.
use crate::cell::{overflow_ridx_insert, overflow_ridx_remove};
use crate::terminal_core::{MODE_AUTO_WRAP, TerminalCore};

/// `try_retroactive_merge` が 1 セルに蓄積できる内容の UTF-8 バイト上限。
/// `handle_print` が `grapheme_buffer` に既に課している 64 コードポイント
/// 上限(64 * 4 バイト)に合わせ、途切れない結合文字ランで 1 セルが無制限に
/// 肥大することを防ぐ。
const MAX_MERGED_CLUSTER_BYTES: usize = 256;

impl TerminalCore {
    /// Apply active charset translation to a codepoint.
    fn translate_charset(&self, cp: u32) -> u32 {
        let charset = if self.active_charset == 0 {
            self.g0_charset
        } else {
            self.g1_charset
        };
        if charset == 1 {
            Self::translate_line_drawing(cp)
        } else {
            cp
        }
    }

    /// DEC Line Drawing translation table (0x5F-0x7E, 32 entries).
    fn translate_line_drawing(cp: u32) -> u32 {
        match cp {
            0x5F => 0x0020, // _ → Blank
            0x60 => 0x25C6, // ` → Diamond
            0x61 => 0x2592, // a → Checkerboard
            0x62 => 0x2409, // b → HT
            0x63 => 0x240C, // c → FF
            0x64 => 0x240D, // d → CR
            0x65 => 0x240A, // e → LF
            0x66 => 0x00B0, // f → Degree
            0x67 => 0x00B1, // g → Plus/minus
            0x68 => 0x2424, // h → NL
            0x69 => 0x240B, // i → VT
            0x6A => 0x2518, // j → Lower right corner
            0x6B => 0x2510, // k → Upper right corner
            0x6C => 0x250C, // l → Upper left corner
            0x6D => 0x2514, // m → Lower left corner
            0x6E => 0x253C, // n → Crossing lines
            0x6F => 0x23BA, // o → Scan 1
            0x70 => 0x23BB, // p → Scan 3
            0x71 => 0x2500, // q → Scan 5 (horizontal line)
            0x72 => 0x23BC, // r → Scan 7
            0x73 => 0x23BD, // s → Scan 9
            0x74 => 0x251C, // t → Left tee
            0x75 => 0x2524, // u → Right tee
            0x76 => 0x2534, // v → Bottom tee
            0x77 => 0x252C, // w → Top tee
            0x78 => 0x2502, // x → Vertical line
            0x79 => 0x2264, // y → Less than or equal
            0x7A => 0x2265, // z → Greater than or equal
            0x7B => 0x03C0, // { → Pi
            0x7C => 0x2260, // | → Not equal
            0x7D => 0x00A3, // } → UK pound
            0x7E => 0x00B7, // ~ → Bullet
            _ => cp,
        }
    }

    /// Blank a wide-pair partner cell (an orphaned spacer or base) in
    /// place: replace its content with a single space at width 1, while
    /// preserving fg/bg/flags/hyperlink (IMPLEMENTATION.md D2), removing
    /// any overflow-table entry for it, and marking the row dirty.
    ///
    /// Callers must already have confirmed the target cell is currently
    /// width 0 or width 2 (the shared partner-blanking precondition) —
    /// this function does not re-check.
    fn blank_wide_pair_partner(&mut self, col: u16, row: u16) {
        let Some(idx) = self.cell_index(col, row) else {
            return;
        };
        if self.ring_cells[idx].is_overflow() && !self.overflow.is_empty() {
            let abs = self.viewport_abs(row) as u32;
            let col32 = col as u32;
            if self.overflow.remove(&(col32, abs)).is_some() {
                overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
            }
        }
        let cell = &mut self.ring_cells[idx];
        cell.set_char(" ");
        cell.width = 1;
        self.mark_row_dirty(row);
    }

    /// R1/R2 (print_handler-local rule, see IMPLEMENTATION.md Shared
    /// Components + task0001 plan Design): before overwriting the cell at
    /// (col, row), whose current width is `old_width`, blank whichever
    /// neighbor would otherwise be orphaned by the overwrite:
    /// - R1: `old_width == 2` (overwriting a wide base) and the spacer at
    ///   col+1 is still width 0 → blank col+1.
    /// - R2: `old_width == 0` (overwriting a spacer) and the base at
    ///   col-1 is still width 2 → blank col-1.
    ///
    /// Callers on the ASCII fast path already have `old_width` from the
    /// same cell access used for the write itself (NFR4: no additional
    /// memory access). No-op when `old_width == 1` (the ordinary case);
    /// callers should skip calling this entirely in that case to avoid
    /// even the redundant re-check.
    fn blank_orphaned_neighbor_before_overwrite(&mut self, col: u16, row: u16, old_width: u8) {
        if old_width == 2 {
            if col + 1 < self.cols {
                if let Some(idx2) = self.cell_index(col + 1, row) {
                    if self.ring_cells[idx2].width == 0 {
                        self.blank_wide_pair_partner(col + 1, row);
                    }
                }
            }
        } else if old_width == 0 && col > 0 {
            if let Some(idx2) = self.cell_index(col - 1, row) {
                if self.ring_cells[idx2].width == 2 {
                    self.blank_wide_pair_partner(col - 1, row);
                }
            }
        }
    }

    /// R3 (print_handler-local rule): before turning (ph_col, row) into a
    /// wide-pair placeholder/spacer, check whether its current content is
    /// itself a wide base (width 2) whose own spacer at ph_col+1 would be
    /// orphaned by the overwrite. Blanks that spacer first — chained
    /// cleanup for when a new wide write's placeholder lands on an
    /// unrelated pair's base.
    fn blank_orphaned_base_before_placeholder(&mut self, ph_col: u16, row: u16) {
        let Some(idx) = self.cell_index(ph_col, row) else {
            return;
        };
        if self.ring_cells[idx].width != 2 {
            return;
        }
        let next_col = ph_col + 1;
        if next_col >= self.cols {
            return;
        }
        if let Some(idx2) = self.cell_index(next_col, row) {
            if self.ring_cells[idx2].width == 0 {
                self.blank_wide_pair_partner(next_col, row);
            }
        }
    }

    /// Write a character/grapheme to grid at cursor, handling wrap and scroll.
    /// Scroll is handled internally via ring buffer.
    fn write_grapheme_to_grid(&mut self, char_str: &str, width: u8) {
        // Handle wrap_pending
        if self.wrap_pending {
            self.wrap_pending = false;
            self.carriage_return();
            self.line_feed();
            let abs = self.viewport_abs(self.cursor.row);
            self.ring_wrapped[abs] = true;
        }

        // Wide char at line end: wrap before printing
        if width == 2 && self.cursor.col >= self.cols.saturating_sub(1) {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.carriage_return();
                self.line_feed();
                let abs = self.viewport_abs(self.cursor.row);
                self.ring_wrapped[abs] = true;
            }
        }

        // Write cell
        let col = self.cursor.col;
        let row = self.cursor.row;
        if let Some(idx) = self.cell_index(col, row) {
            let old_width = self.ring_cells[idx].width;
            if old_width != 1 {
                // R1/R2 (FR1/FR2): blank an orphaned wide-pair neighbor
                // before this overwrite lands.
                self.blank_orphaned_neighbor_before_overwrite(col, row, old_width);
            }
            let abs = self.viewport_abs(row) as u32;
            let cell = &mut self.ring_cells[idx];
            cell.set_char(char_str);
            let col32 = col as u32;
            if cell.is_overflow() {
                self.overflow.insert((col32, abs), char_str.to_string());
                overflow_ridx_insert(&mut self.overflow_ridx, abs, col32);
            } else if !self.overflow.is_empty() {
                if self.overflow.remove(&(col32, abs)).is_some() {
                    overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
                }
            }
            cell.width = width;
            cell.fg = self.cursor.fg;
            cell.bg = self.cursor.bg;
            cell.flags = self.cursor.flags;
            cell.hyperlink_id = self.active_hyperlink_id;
            self.mark_row_dirty(row);
            // Track the base cell just written as the merge target for a
            // subsequent standalone-arriving zero-width character (FR1).
            self.last_write = Some((col, row));
        }

        // Placeholder for width-2 characters
        if width == 2 && col + 1 < self.cols {
            // R3 (FR3): a chained-cleanup blank if col+1 currently holds
            // an unrelated wide pair's base, before it becomes this pair's
            // placeholder.
            self.blank_orphaned_base_before_placeholder(col + 1, row);
            if let Some(idx) = self.cell_index(col + 1, row) {
                let abs = self.viewport_abs(row) as u32;
                let ph = &mut self.ring_cells[idx];
                ph.char_data = [0; 16];
                ph.char_len = 0;
                ph.width = 0;
                ph.fg = self.cursor.fg;
                ph.bg = self.cursor.bg;
                ph.flags = self.cursor.flags;
                ph.hyperlink_id = self.active_hyperlink_id;
                if !self.overflow.is_empty() {
                    let col1_32 = (col + 1) as u32;
                    if self.overflow.remove(&(col1_32, abs)).is_some() {
                        overflow_ridx_remove(&mut self.overflow_ridx, abs, col1_32);
                    }
                }
            }
        }

        // Advance cursor
        let new_col = col as u32 + width as u32;
        if new_col >= self.cols as u32 {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.cursor.col = self.cols - 1;
                self.wrap_pending = true;
            }
        } else {
            self.cursor.col = new_col as u16;
        }
    }

    /// ASCII fast path: direct byte write without string allocation.
    fn handle_print_ascii(&mut self, cp: u32) {
        let byte = cp as u8;
        let col = self.cursor.col;
        let row = self.cursor.row;
        if let Some(idx) = self.cell_index(col, row) {
            // D4 (NFR4): single branch gates the wide-pair partner-blanking
            // rule (R1/R2); the ordinary width-1 overwrite (common case)
            // does no extra work beyond this one width read, which reuses
            // the cache line already touched for the write below.
            let old_width = self.ring_cells[idx].width;
            if old_width != 1 {
                self.blank_orphaned_neighbor_before_overwrite(col, row, old_width);
            }
            let cell = &mut self.ring_cells[idx];
            cell.char_data[0] = byte;
            // Skip zeroing char_data[1..]: char_len=1 ensures only byte 0 is read
            cell.char_len = 1;
            cell.width = 1;
            cell.fg = self.cursor.fg;
            cell.bg = self.cursor.bg;
            cell.flags = self.cursor.flags;
            cell.hyperlink_id = self.active_hyperlink_id;
            // Only check overflow table when it has entries (common case: empty)
            if !self.overflow.is_empty() {
                let abs = self.viewport_abs(row) as u32;
                let col32 = col as u32;
                if self.overflow.remove(&(col32, abs)).is_some() {
                    overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
                }
            }
            self.mark_row_dirty(row);
            // Track the base cell just written (FR1); see write_grapheme_to_grid.
            self.last_write = Some((col, row));
        }

        let new_col = col + 1;
        if new_col < self.cols {
            self.cursor.col = new_col;
        } else if self.get_mode(MODE_AUTO_WRAP) {
            self.cursor.col = self.cols - 1;
            self.wrap_pending = true;
        }
    }

    /// Slow path: handles charWidth, charset translation, wrap.
    fn handle_print_slow(&mut self, cp: u32) {
        let width = crate::unicode::char_width(cp);
        let translated = self.translate_charset(cp);
        let mut buf = [0u8; 4];
        let ch = char::from_u32(translated).unwrap_or(' ');
        let s = ch.encode_utf8(&mut buf);
        self.write_grapheme_to_grid(s, width);
    }

    /// Read a cell's full current content (inline or overflow) as an owned
    /// `String`. Unlike `get_cell_char` (viewport-relative row), this takes
    /// an already-resolved absolute row, matching the internal bookkeeping
    /// used by the merge helpers below.
    fn cell_content_at(&self, idx: usize, col: u16, abs: u32) -> String {
        let cell = &self.ring_cells[idx];
        if cell.is_overflow() {
            self.overflow
                .get(&(col as u32, abs))
                .cloned()
                .unwrap_or_default()
        } else {
            cell.get_char_inline().unwrap_or(" ").to_string()
        }
    }

    /// Attempt to merge a standalone-arriving zero-width character
    /// (VARIATION_SEL or COMBINING, per `crate::unicode::classify_codepoint`)
    /// into the most recently written grid cell (FR1). Returns `true` if the
    /// character was merged, `false` if there was no valid merge target and
    /// the character was dropped entirely — no grid write, no cursor
    /// movement (FR4).
    fn try_retroactive_merge(&mut self, cp: u32) -> bool {
        let Some((mut col, row)) = self.last_write else {
            return false; // FR4: nothing written yet on this screen.
        };

        // FR3: spacer traversal. If the tracked position holds a wide-cell
        // spacer, the real merge target is its base cell one column left.
        if let Some(idx) = self.cell_index(col, row) {
            if self.ring_cells[idx].width == 0 {
                if col == 0 {
                    return false; // Orphaned spacer with no base: drop (FR4).
                }
                col -= 1;
            }
        }

        let Some(idx) = self.cell_index(col, row) else {
            return false; // FR4: tracked position no longer addressable.
        };

        let abs = self.viewport_abs(row) as u32;
        let col32 = col as u32;
        let mut content = self.cell_content_at(idx, col, abs);

        // 上限に達したら以降のマークは「消費して破棄」する。これが無いと
        // マーク 1 個ごとにセル内容全体を複製するため合計 O(N^2) になり、
        // セル内容長にも上限が無くなる。
        if content.len() >= MAX_MERGED_CLUSTER_BYTES {
            return true;
        }

        let mut buf = [0u8; 4];
        let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
        content.push_str(ch.encode_utf8(&mut buf));

        let old_width = self.ring_cells[idx].width;

        // Write the grown content back (may push inline content to overflow;
        // mirrors the bookkeeping in write_grapheme_to_grid / set_cell).
        let cell = &mut self.ring_cells[idx];
        cell.set_char(&content);
        if cell.is_overflow() {
            self.overflow.insert((col32, abs), content);
            overflow_ridx_insert(&mut self.overflow_ridx, abs, col32);
        } else if !self.overflow.is_empty() && self.overflow.remove(&(col32, abs)).is_some() {
            overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
        }
        self.mark_row_dirty(row);
        self.last_write = Some((col, row));

        // FR2: VS16 retroactively widens a width-1 base cell to width 2.
        if cp == 0xFE0F && old_width == 1 {
            self.widen_after_merge(col, row);
        }

        true
    }

    /// After a VS16 (U+FE0F) merge onto a width-1 base cell, retroactively
    /// widen it to width 2: create a spacer cell at the next column and
    /// advance the cursor by one (FR2). If the base cell sits in the last
    /// column, apply the same end-of-line semantics the existing wide-char
    /// write path uses (AC-6).
    fn widen_after_merge(&mut self, col: u16, row: u16) {
        let last_col = self.cols.saturating_sub(1);
        if col == last_col {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.relocate_widened_base_via_wrap(col, row);
            } else {
                // No room for a spacer and auto-wrap is off: widen in place
                // with no placeholder, matching the existing no-autowrap
                // wide-char end-of-line quirk. Cursor stays pinned.
                if let Some(idx) = self.cell_index(col, row) {
                    self.ring_cells[idx].width = 2;
                }
            }
            return;
        }

        // Common case: room for a spacer at col + 1.
        let Some(base_idx) = self.cell_index(col, row) else {
            return;
        };
        self.ring_cells[base_idx].width = 2;
        let (fg, bg, flags, hyperlink_id) = {
            let base = &self.ring_cells[base_idx];
            (base.fg, base.bg, base.flags, base.hyperlink_id)
        };
        // R3 (FR3/FR4): a chained-cleanup blank if col+1 currently holds
        // an unrelated wide pair's base, before it becomes this widened
        // cell's spacer.
        self.blank_orphaned_base_before_placeholder(col + 1, row);
        let abs = self.viewport_abs(row) as u32;
        if let Some(sp_idx) = self.cell_index(col + 1, row) {
            let sp = &mut self.ring_cells[sp_idx];
            sp.char_data = [0; 16];
            sp.char_len = 0;
            sp.width = 0;
            sp.fg = fg;
            sp.bg = bg;
            sp.flags = flags;
            sp.hyperlink_id = hyperlink_id;
            let col1_32 = (col + 1) as u32;
            if !self.overflow.is_empty() && self.overflow.remove(&(col1_32, abs)).is_some() {
                overflow_ridx_remove(&mut self.overflow_ridx, abs, col1_32);
            }
        }
        self.mark_row_dirty(row);

        // Advance the cursor by 1: the VS16 widening consumes one more column.
        let new_col = col as u32 + 2;
        if new_col >= self.cols as u32 {
            if self.get_mode(MODE_AUTO_WRAP) {
                self.cursor.col = self.cols - 1;
                self.wrap_pending = true;
            }
        } else {
            self.cursor.col = new_col as u16;
        }
        self.last_write = Some((col, row));
    }

    /// Relocate a base cell that just widened to width 2 while sitting in
    /// the last column: move its content to the start of the next row
    /// (mirroring the pre-emptive wrap the normal wide-char write path
    /// takes), mark the new row as a wrap continuation of the old one, and
    /// leave the cursor past the new spacer (AC-6).
    fn relocate_widened_base_via_wrap(&mut self, old_col: u16, old_row: u16) {
        let Some(old_idx) = self.cell_index(old_col, old_row) else {
            return;
        };
        let old_abs = self.viewport_abs(old_row) as u32;
        let content = self.cell_content_at(old_idx, old_col, old_abs);
        let old_cell = self.ring_cells[old_idx];
        let (fg, bg, flags, hyperlink_id) = (
            old_cell.fg,
            old_cell.bg,
            old_cell.flags,
            old_cell.hyperlink_id,
        );

        // Vacate the old cell: its content is moving to the next row.
        self.ring_cells[old_idx] = self.bce_cell();
        if !self.overflow.is_empty() && self.overflow.remove(&(old_col as u32, old_abs)).is_some() {
            overflow_ridx_remove(&mut self.overflow_ridx, old_abs, old_col as u32);
        }
        self.mark_row_dirty(old_row);

        // Wrap to the start of the next row (same primitives
        // write_grapheme_to_grid uses for its own pre-emptive wrap).
        self.carriage_return();
        self.line_feed();
        let new_row = self.cursor.row;
        let new_abs = self.viewport_abs(new_row) as u32;
        self.ring_wrapped[new_abs as usize] = true;

        // R1/R2 (FR1/FR2, Design item 4): the relocated base write at the
        // new row's col 0 may itself orphan a neighbor if that row already
        // held wide-pair remnants.
        if let Some(idx) = self.cell_index(0, new_row) {
            let old_width = self.ring_cells[idx].width;
            if old_width != 1 {
                self.blank_orphaned_neighbor_before_overwrite(0, new_row, old_width);
            }
        }
        if let Some(idx) = self.cell_index(0, new_row) {
            let cell = &mut self.ring_cells[idx];
            cell.set_char(&content);
            if cell.is_overflow() {
                self.overflow.insert((0, new_abs), content.clone());
                overflow_ridx_insert(&mut self.overflow_ridx, new_abs, 0);
            }
            cell.width = 2;
            cell.fg = fg;
            cell.bg = bg;
            cell.flags = flags;
            cell.hyperlink_id = hyperlink_id;
        }
        // R3 (FR3, Design item 4): the relocated spacer write at col 1 may
        // land on another pair's base, orphaning that pair's own spacer.
        self.blank_orphaned_base_before_placeholder(1, new_row);
        if let Some(idx) = self.cell_index(1, new_row) {
            let sp = &mut self.ring_cells[idx];
            sp.char_data = [0; 16];
            sp.char_len = 0;
            sp.width = 0;
            sp.fg = fg;
            sp.bg = bg;
            sp.flags = flags;
            sp.hyperlink_id = hyperlink_id;
        }
        self.mark_row_dirty(new_row);

        self.cursor.col = 2;
        self.wrap_pending = false;
        self.last_write = Some((0, new_row));
    }
}

impl TerminalCore {
    /// Process a single codepoint for printing.
    /// Returns 0 always (scroll handled internally via ring buffer).
    pub fn handle_print(&mut self, cp: u32) -> u8 {
        // Kitty Unicode placeholder suppression (U+10EEEE + combining marks).
        // kitten icat uses these to reserve cell positions for image display.
        // eMterm handles images via APC/viewer, so placeholders are discarded.
        if cp == 0x10EEEE {
            self.flush_grapheme_buffer();
            self.kitty_placeholder_active = true;
            return 0;
        }
        if self.kitty_placeholder_active {
            let props_check = crate::unicode::classify_codepoint(cp);
            if props_check & crate::unicode::COMBINING != 0 {
                return 0; // Skip combining marks attached to placeholder
            }
            self.kitty_placeholder_active = false;
        }

        // Safety: flush if buffer exceeds max size
        if self.grapheme_buffer.len() >= 64 {
            self.flush_grapheme_buffer();
        }

        let props = crate::unicode::classify_codepoint(cp);

        if !self.grapheme_buffer.is_empty() {
            // Buffer non-empty: check if cp extends the cluster
            if cp == 0x200D {
                self.grapheme_buffer.push(cp);
                return 0;
            }
            if props & crate::unicode::VARIATION_SEL != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }
            if props & crate::unicode::SKIN_TONE != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }
            if props & crate::unicode::REGIONAL_IND != 0 {
                if self.grapheme_buffer.len() == 1 {
                    let buf0 = self.grapheme_buffer[0];
                    if (0x1F1E6..=0x1F1FF).contains(&buf0) {
                        self.grapheme_buffer.push(cp);
                        self.flush_grapheme_buffer();
                        return 0;
                    }
                }
            }
            if let Some(&last) = self.grapheme_buffer.last() {
                if last == 0x200D && (props & crate::unicode::EXT_PICTOGRAPHIC != 0) {
                    self.grapheme_buffer.push(cp);
                    return 0;
                }
            }
            if props & crate::unicode::COMBINING != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }

            // Does not extend: flush and fall through
            self.flush_grapheme_buffer();
        } else {
            // Buffer empty: check if cp starts buffering
            if props & (crate::unicode::EXT_PICTOGRAPHIC | crate::unicode::REGIONAL_IND) != 0 {
                self.grapheme_buffer.push(cp);
                return 0;
            }
            // Standalone-arriving zero-width character (VARIATION_SEL or
            // COMBINING) that does not start a buffered cluster: retroactively
            // merge it into the most recently written cell instead of falling
            // through to the slow path, which would overwrite the cursor cell
            // (FR1-FR4). Always consumed here, merged or dropped.
            if props & (crate::unicode::VARIATION_SEL | crate::unicode::COMBINING) != 0 {
                self.try_retroactive_merge(cp);
                return 0;
            }
        }

        // ASCII fast path
        if cp >= 0x20
            && cp < 0x7F
            && !self.wrap_pending
            && self.active_charset == 0
            && self.g0_charset == 0
        {
            let new_col = self.cursor.col + 1;
            if new_col < self.cols || self.get_mode(MODE_AUTO_WRAP) {
                self.handle_print_ascii(cp);
                return 0;
            }
        }

        // Slow path
        self.handle_print_slow(cp);
        0
    }

    /// Flush the grapheme buffer, writing the accumulated cluster to the grid.
    /// Scroll handled internally via ring buffer.
    pub fn flush_grapheme_buffer(&mut self) -> u8 {
        if self.grapheme_buffer.is_empty() {
            return 0;
        }

        let mut cluster = String::with_capacity(self.grapheme_buffer.len() * 4);
        let mut has_fe0e = false;
        let mut has_fe0f = false;

        for &cp in &self.grapheme_buffer {
            if cp == 0xFE0E {
                has_fe0e = true;
            }
            if cp == 0xFE0F {
                has_fe0f = true;
            }
            if let Some(ch) = char::from_u32(cp) {
                cluster.push(ch);
            }
        }

        let width: u8 = if has_fe0e {
            1
        } else if has_fe0f {
            2
        } else if self.grapheme_buffer.len() == 1 {
            if crate::unicode::is_emoji_presentation(self.grapheme_buffer[0]) {
                2
            } else {
                crate::unicode::char_width(self.grapheme_buffer[0])
            }
        } else {
            2
        };

        self.grapheme_buffer.clear();
        self.write_grapheme_to_grid(&cluster, width);
        0
    }
}

#[cfg(test)]
mod tests;
