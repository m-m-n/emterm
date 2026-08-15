/// PTY data dispatch: process_pty_data, dispatch_action, and buffer switch detection.
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    /// Process raw PTY data through the WASM parser and dispatch internally.
    ///
    /// Uses a two-phase approach for performance:
    /// 1. **Fast path**: When the parser is in Ground state with no pending grapheme
    ///    buffer, consecutive printable ASCII bytes (0x20-0x7E) and C0 controls
    ///    (\r, \n) are processed directly without going through the parser state
    ///    machine or action dispatch. This eliminates ~10 levels of indirection
    ///    per character for the common case.
    /// 2. **Slow path**: Non-ASCII bytes, escape sequences, and other special
    ///    bytes fall through to the standard parser.
    ///
    /// Returns the number of bytes consumed. If a buffer switch action is queued
    /// (mode 47/1047/1049), processing stops early so the caller can route
    /// remaining data to the correct core.
    pub fn process_pty_data(&mut self, data: &[u8]) -> usize {
        let mut parser = std::mem::take(&mut self.parser);
        let mut pos: usize = 0;
        let len = data.len();

        // Pre-check conditions for ASCII fast path eligibility
        let can_fast_ascii =
            self.active_charset == 0 && self.g0_charset == 0 && !self.kitty_placeholder_active;

        // === Bulk scroll skip-ahead ===
        // When data contains far more newlines than the viewport,
        // skip processing lines that would scroll off-screen anyway.
        // Keep only `rows` trailing lines to fill the viewport, avoiding
        // expensive per-line scroll_up_internal calls for scrollback.
        // This reduces 10M lines of `seq` to ~40 lines of actual processing.
        let rows_usize = self.rows as usize;
        if can_fast_ascii
            && parser.is_ground_clean()
            && self.grapheme_buffer.is_empty()
            && self.cursor.row >= self.scroll_region_bottom
            && self.scroll_region_top == 0
            && self.scroll_region_bottom == self.rows.saturating_sub(1)
            && len > self.ring_capacity * 4
        {
            // Single-pass scan: verify all bytes are fast-path eligible and count newlines
            let mut newline_count = 0usize;
            let mut all_fast = true;
            for &b in data {
                match b {
                    0x0A => newline_count += 1,
                    0x07 | 0x08 | 0x09 | 0x0B | 0x0C | 0x0D | 0x20..=0x7E => {}
                    _ => {
                        all_fast = false;
                        break;
                    }
                }
            }

            if all_fast && newline_count > rows_usize {
                // Skip to leave only viewport rows for fast-path processing.
                // Scrollback is cleared (acceptable during high-throughput output).
                let skip_lines = newline_count - rows_usize;
                let mut lines_seen = 0usize;
                let mut skip_pos = 0usize;
                for (i, &b) in data.iter().enumerate() {
                    if b == 0x0A {
                        lines_seen += 1;
                        if lines_seen >= skip_lines {
                            skip_pos = i + 1;
                            break;
                        }
                    }
                }

                // Bulk-reset the ring buffer: clear everything, push fresh viewport rows
                self.ring_head = 0;
                self.ring_size = 0;
                self.scroll_event = None;
                self.overflow.clear();
                self.overflow_ridx.clear();
                let bg = self.cursor.bg;
                for _ in 0..self.rows {
                    self.ring_push_blank(bg);
                }
                self.cursor.row = self.rows.saturating_sub(1);
                self.cursor.col = 0;
                self.wrap_pending = false;
                self.mark_all_dirty();
                pos = skip_pos;
            }
        }

        while pos < len {
            // === ASCII fast path ===
            // Process runs of printable ASCII + CR/LF directly, bypassing parser
            if can_fast_ascii && parser.is_ground_clean() && self.grapheme_buffer.is_empty() {
                let fast_start = pos;
                while pos < len {
                    let b = data[pos];
                    match b {
                        // Printable ASCII: inline handle_print_ascii logic
                        0x20..=0x7E => {
                            if self.wrap_pending {
                                // Wrap pending: fall through to slow path for correct handling
                                break;
                            }
                            let col = self.cursor.col;
                            if let Some(idx) = self.cell_index(col, self.cursor.row) {
                                // NFR1: one read of a field in the cell record
                                // already being written below (same cache
                                // line) plus one untaken branch on the common
                                // width-1 case — no allocation, no extra pass
                                // over the input buffer, no non-inlinable
                                // per-byte call. Mirrors the budget comment on
                                // handle_print_ascii (print_handler.rs), the
                                // slow ASCII writer this fast path must stay
                                // in parity with (FR3, D-1).
                                let old_width = self.ring_cells[idx].width;
                                // FR2/NFR1 (task0004): the overflow marker
                                // is read from the SAME cell record
                                // `old_width` just touched, BEFORE the
                                // write below clears it (char_len becomes
                                // 1) — a read placed after the write always
                                // observes false and silently skips the
                                // cleanup.
                                let was_overflow = self.ring_cells[idx].is_overflow();
                                if old_width != 1 {
                                    // R1/R2 (FR1/FR2): blank an orphaned
                                    // wide-pair neighbor before this overwrite
                                    // lands. The repair only ever mutates a
                                    // neighboring column, so `idx` stays valid
                                    // for the write below.
                                    self.blank_orphaned_neighbor_before_overwrite(
                                        col,
                                        self.cursor.row,
                                        old_width,
                                    );
                                }
                                let cell = &mut self.ring_cells[idx];
                                cell.char_data[0] = b;
                                cell.char_len = 1;
                                cell.width = 1;
                                cell.fg = self.cursor.fg;
                                cell.bg = self.cursor.bg;
                                cell.flags = self.cursor.flags;
                                cell.hyperlink_id = self.active_hyperlink_id;
                                // FR2: keep the overflow table and its
                                // reverse index consistent when the
                                // overwritten cell's long content is replaced
                                // by this single ASCII byte (mirrors
                                // handle_print_ascii's overflow cleanup).
                                // Gated on the overwritten cell's OWN
                                // pre-write marker (task0004), not on "the
                                // table is non-empty anywhere in the ring":
                                // the common ASCII case does no table
                                // access and no absolute-row computation.
                                //
                                // Invariant this gate depends on (FR4/FR5,
                                // task0001): an overflow-table entry exists
                                // at a (column, absolute row) key only while
                                // the cell at that key reports
                                // overflow-bound (its marker set).
                                // Obligation: because this gate no longer
                                // sweeps the whole ring for stale entries,
                                // every write anywhere in the print
                                // subsystem that clears a cell's overflow
                                // marker owns removing that cell's own
                                // table entry itself.
                                if was_overflow {
                                    let abs = self.viewport_abs(self.cursor.row) as u32;
                                    let col32 = col as u32;
                                    if self.overflow.remove(&(col32, abs)).is_some() {
                                        crate::cell::overflow_ridx_remove(
                                            &mut self.overflow_ridx,
                                            abs,
                                            col32,
                                        );
                                    }
                                }
                                self.mark_row_dirty(self.cursor.row);
                                // Track the base cell just written (FR1); this
                                // fast path bypasses handle_print_ascii, which
                                // does the same bookkeeping on the slow path.
                                self.last_write = Some((col, self.cursor.row));
                            }
                            let new_col = col + 1;
                            if new_col < self.cols {
                                self.cursor.col = new_col;
                            } else if self.get_mode(crate::terminal_core::MODE_AUTO_WRAP) {
                                self.cursor.col = self.cols - 1;
                                self.wrap_pending = true;
                            }
                            pos += 1;
                        }
                        // CR: carriage return
                        0x0D => {
                            self.cursor.col = 0;
                            self.wrap_pending = false;
                            // Explicit cursor movement invalidates the merge
                            // target (FR4); this fast path bypasses dispatch_action's
                            // blanket invalidation for Execute actions.
                            self.last_write = None;
                            pos += 1;
                        }
                        // LF/VT/FF: line feed
                        0x0A | 0x0B | 0x0C => {
                            self.line_feed();
                            self.wrap_pending = false;
                            pos += 1;
                        }
                        // BS: backspace
                        0x08 => {
                            self.cursor.col = self.cursor.col.saturating_sub(1);
                            self.wrap_pending = false;
                            self.last_write = None;
                            pos += 1;
                        }
                        // HT: horizontal tab
                        0x09 => {
                            self.cursor.col = self.next_tab_stop(self.cursor.col);
                            self.wrap_pending = false;
                            self.last_write = None;
                            pos += 1;
                        }
                        // BEL
                        0x07 => {
                            self.fire_bell_callback();
                            pos += 1;
                        }
                        // Anything else: fall to slow path
                        _ => break,
                    }
                }
                // If we processed any bytes in the fast path, continue the loop
                if pos > fast_start {
                    continue;
                }
            }

            // === Slow path: use parser for escape sequences, UTF-8, etc. ===
            let remaining = &data[pos..];
            let consumed = parser.parse_interruptible(remaining, |action| {
                self.dispatch_action(action);
                !self.has_pending_buffer_switch() && !self.cursor_just_shown
            });
            pos += consumed;

            // If parser stopped due to buffer switch, break
            if self.has_pending_buffer_switch() {
                break;
            }
            // If cursor just became visible (hidden→visible transition),
            // break to let JS render the current state before processing
            // the next update pass (e.g., vim's search wrap message).
            if self.cursor_just_shown {
                self.cursor_just_shown = false;
                break;
            }
            // If parser consumed 0 bytes, avoid infinite loop
            if consumed == 0 {
                break;
            }
        }

        self.parser = parser;
        pos
    }

    /// Check if mode_actions contains a pending buffer switch (action codes 1, 2, or 3).
    /// Skips TS_FALLBACK entries (3-byte: 0xFF/0xFE + mode_lo + mode_hi).
    pub(crate) fn has_pending_buffer_switch(&self) -> bool {
        let actions = &self.mode_actions;
        let mut i = 0;
        while i < actions.len() {
            let code = actions[i];
            if code == 0xFF || code == 0xFE {
                // TS_FALLBACK: 3-byte entry, skip
                i += 3;
            } else {
                if code >= 1 && code <= 3 {
                    return true;
                }
                i += 1;
            }
        }
        false
    }

    /// Route a single ParsedAction to the appropriate handler.
    fn dispatch_action(&mut self, action: crate::parser_types::ParsedAction) {
        use crate::parser_types::ParsedAction;
        match action {
            ParsedAction::Print(ch) => {
                self.handle_print(ch as u32);
            }
            // For all non-Print actions, flush the grapheme buffer first.
            // This ensures any accumulated emoji/pictographic codepoints are
            // written to the grid at the correct cursor position BEFORE
            // cursor movements, erases, or other operations change the state.
            //
            // Every non-Print action also invalidates the retroactive zero-
            // width-merge target (`last_write`): conservatively, ANY of these
            // (cursor movement, erase, resize, mode/buffer switch, ...) may
            // displace or repurpose the most recently written cell (FR4).
            // The flush above may re-set `last_write` to the just-flushed
            // cluster's position, so invalidation runs after it.
            ParsedAction::Execute(byte) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.last_write = None;
                self.handle_execute_internal(byte);
            }
            ParsedAction::CsiDispatch {
                params,
                param_count,
                intermediates,
                intermediate_count,
                final_byte,
            } => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.last_write = None;
                self.handle_csi_internal(
                    &params[..param_count as usize],
                    &intermediates[..intermediate_count as usize],
                    final_byte,
                );
            }
            ParsedAction::EscDispatch {
                intermediate,
                final_byte,
            } => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.last_write = None;
                self.handle_esc_internal(intermediate, final_byte);
            }
            ParsedAction::OscDispatch { param, data } => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.last_write = None;
                self.handle_osc_internal(param, &data);
            }
            ParsedAction::ApcDispatch(payload) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.last_write = None;
                use crate::apc_handler::KittyApcResult;
                let result = self.handle_kitty_apc(&payload);
                // Forward to backend for all except query (which needs no image processing)
                if !matches!(result, KittyApcResult::QueryHandled) {
                    self.fire_apc_callback(&payload);
                }
            }
            ParsedAction::DcsDispatch(payload) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.last_write = None;
                self.fire_dcs_callback(&payload);
            }
        }
    }
}

// ── ascii-fast-path-wide-pair-cleanup (task0001): dispatch fast-path
// write-step D2 repair ─────────────────────────────────────────────
//
// Every scenario below delivers its setup and its overwriting ASCII byte
// in SEPARATE `process_pty_data` calls (see task0001 plan Design, "Why the
// tests must split their input"): the fast path is only entered when the
// parser is ground-clean and the grapheme buffer is empty at the START of
// a `process_pty_data` call, so a single call beginning with non-ASCII
// content drives its whole remainder — including any trailing ASCII —
// through the parser-driven slow path instead.
#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // AC-1 (FR1, FR6, TS1): a fullwidth character written by one dispatch
    // call, overwritten at column 0 by an ASCII letter in a following
    // dispatch call (CR + letter), leaves no width-0 spacer orphaned.
    #[test]
    fn process_pty_data_ascii_overwrites_wide_base_blanks_orphan_spacer() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.process_pty_data("\u{4E16}".as_bytes()); // '世': base@0(w2)/spacer@1(w0)
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_width(1, 0), 0);

        core.process_pty_data(b"\rA"); // CR then 'A': fast-path-eligible call

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert_eq!(core.get_cell_char(1, 0), " "); // orphaned spacer, blanked
        assert_eq!(core.get_cell_width(1, 0), 1);
    }

    // AC-2 (FR1, TS3): with a fullwidth character occupying columns 0-1
    // and the cursor moved onto the spacer half (col1) by its own dispatch
    // call, a following dispatch call carrying an ASCII letter leaves col1
    // holding that letter and col0 blanked — no width-2 base survives
    // without its spacer.
    #[test]
    fn process_pty_data_ascii_overwrites_wide_spacer_blanks_orphan_base() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.process_pty_data("\u{4E16}".as_bytes()); // base@0(w2)/spacer@1(w0)
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_width(1, 0), 0);

        core.process_pty_data(b"\x1b[1;2H"); // CUP row1,col2 (1-indexed) -> (col1,row0)
        core.process_pty_data(b"B"); // fast-path-eligible call

        assert_eq!(core.get_cell_char(1, 0), "B");
        assert_eq!(core.get_cell_width(1, 0), 1);
        assert_eq!(core.get_cell_char(0, 0), " "); // orphaned base, blanked
        assert_eq!(core.get_cell_width(0, 0), 1);
    }

    // AC-3 (FR2, TS4): overwriting an overflow-table-bound wide-pair base
    // via the fast path removes the overflow entry and its reverse-index
    // companion; the resulting state matches what the slow ASCII writer
    // produces for the same overwrite (see
    // print_handler::tests::test_handle_print_ascii_overwrite_overflow_base_blanks_spacer,
    // which exercises handle_print_ascii directly for that parity claim).
    #[test]
    fn process_pty_data_ascii_overwrites_overflow_base_removes_overflow_entry() {
        let mut core = TerminalCore::new(10, 3, 0);
        // ZWJ family emoji (7 codepoints / 25 UTF-8 bytes): merges into a
        // single width-2 cluster whose base exceeds the 16-byte inline cell
        // capacity and goes to the overflow side table. Buffers until CR
        // forces the flush in its own dispatch call.
        core.process_pty_data(
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}".as_bytes(),
        );
        core.process_pty_data(b"\r"); // flush -> base@0(w2, overflow)/spacer@1(w0); CR -> col0

        let idx0 = core.cell_index(0, 0).expect("col0 row0 in bounds");
        assert!(core.ring_cells[idx0].is_overflow());
        let abs = core.viewport_abs(0) as u32;
        assert!(core.overflow.contains_key(&(0u32, abs)));
        assert!(
            core.overflow_ridx
                .get(&abs)
                .map(|cols| cols.contains(&0u32))
                .unwrap_or(false)
        );
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_width(1, 0), 0);

        core.process_pty_data(b"A"); // fast-path-eligible call

        assert_eq!(core.get_cell_char(0, 0), "A");
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert_eq!(core.get_cell_char(1, 0), " "); // orphaned spacer, blanked
        assert_eq!(core.get_cell_width(1, 0), 1);
        assert!(!core.overflow.contains_key(&(0u32, abs)));
        assert!(!core.overflow_ridx.contains_key(&abs));
    }

    // AC-4 (FR3, TS2): a core fed [fullwidth char, CR, ASCII letter] in one
    // dispatch call (parser-driven slow path throughout, already correct)
    // and a core fed the same bytes split so the ASCII tail begins a
    // fast-path-eligible call end with identical grid contents, widths and
    // overflow-table state.
    #[test]
    fn process_pty_data_split_fast_path_matches_single_call_slow_path() {
        let mut stream = "\u{4E16}".as_bytes().to_vec();
        stream.push(b'\r');
        stream.push(b'A');

        let mut core_slow = TerminalCore::new(10, 3, 0);
        core_slow.process_pty_data(&stream); // one call: slow path throughout

        let mut core_fast = TerminalCore::new(10, 3, 0);
        core_fast.process_pty_data("\u{4E16}".as_bytes()); // setup, own call
        core_fast.process_pty_data(b"\rA"); // fast-path-eligible call

        for col in 0..10u16 {
            assert_eq!(
                core_fast.get_cell_char(col, 0),
                core_slow.get_cell_char(col, 0),
                "col {col}: char mismatch"
            );
            assert_eq!(
                core_fast.get_cell_width(col, 0),
                core_slow.get_cell_width(col, 0),
                "col {col}: width mismatch"
            );
        }
        assert_eq!(core_fast.overflow, core_slow.overflow);
        assert_eq!(core_fast.overflow_ridx, core_slow.overflow_ridx);
    }

    // AC-5 (FR4, FR7, NFR1, TS5): on a grid holding no wide cells, a
    // pure-ASCII stream driven through the dispatch fast path produces the
    // same grid as the same stream driven one character at a time through
    // handle_print (which does not enter this fast path at all), and every
    // touched cell has width 1.
    #[test]
    fn process_pty_data_pure_ascii_fast_path_matches_direct_handle_print() {
        let text: &[u8] = b"Hello, World! 123";

        let mut core_fast = TerminalCore::new(40, 3, 0);
        core_fast.process_pty_data(text);

        let mut core_direct = TerminalCore::new(40, 3, 0);
        for &b in text {
            core_direct.handle_print(b as u32);
        }

        for col in 0..40u16 {
            assert_eq!(
                core_fast.get_cell_char(col, 0),
                core_direct.get_cell_char(col, 0),
                "col {col}: char mismatch"
            );
            assert_eq!(
                core_fast.get_cell_width(col, 0),
                1,
                "col {col}: not width 1"
            );
            assert_eq!(
                core_direct.get_cell_width(col, 0),
                1,
                "col {col}: not width 1"
            );
        }
    }

    // AC-6 (NFR3): overwriting an orphan width-0 cell at column 0 (no left
    // neighbor to consult) completes without panic or out-of-range access,
    // and does not blank anything outside the overwritten cell itself.
    #[test]
    fn process_pty_data_ascii_overwrites_width0_at_col0_no_panic() {
        let mut core = TerminalCore::new(10, 3, 0);
        let idx = core.cell_index(0, 0).unwrap();
        core.ring_cells[idx].char_data = [0; 16];
        core.ring_cells[idx].char_len = 0;
        core.ring_cells[idx].width = 0; // adversarial: no width-2 base exists

        core.process_pty_data(b"Z");

        assert_eq!(core.get_cell_char(0, 0), "Z");
        assert_eq!(core.get_cell_width(0, 0), 1);
    }

    // AC-6 (NFR3): overwriting a width-2 base sitting in the last column of
    // a row (its would-be spacer column does not exist) completes without
    // panic or out-of-range access.
    #[test]
    fn process_pty_data_ascii_overwrites_width2_base_at_last_column_no_panic() {
        let mut core = TerminalCore::new(10, 3, 0);
        let last_col = 9u16;
        let idx = core.cell_index(last_col, 0).unwrap();
        core.ring_cells[idx].width = 2; // adversarial: no in-bounds spacer exists
        core.set_cursor(last_col, 0);

        core.process_pty_data(b"Z");

        assert_eq!(core.get_cell_char(last_col, 0), "Z");
        assert_eq!(core.get_cell_width(last_col, 0), 1);
    }

    // AC-6 (NFR3): overwriting a width-0 cell whose left neighbor is NOT a
    // width-2 base (combining-mark residue rather than a real spacer)
    // leaves that left neighbor untouched — the repair keys off the
    // wide-pair relationship, not width 0 alone.
    #[test]
    fn process_pty_data_ascii_overwrites_width0_without_wide_left_neighbor_leaves_it_untouched() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.handle_print(b'X' as u32); // col0 = 'X', width 1, cursor -> col1
        let idx = core.cell_index(1, 0).unwrap();
        core.ring_cells[idx].width = 0; // adversarial: col0 is width 1, not 2
        core.set_cursor(1, 0);

        core.process_pty_data(b"Y");

        assert_eq!(core.get_cell_char(0, 0), "X"); // left neighbor untouched
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert_eq!(core.get_cell_char(1, 0), "Y");
        assert_eq!(core.get_cell_width(1, 0), 1);
    }

    // ── task0004: overflow cleanup gated on the overwritten cell ──────
    //
    // The guard moves from "the overflow table is non-empty anywhere in
    // the ring" to "the overwritten cell's own pre-write overflow marker
    // is set". The marker must be read from the cell record BEFORE the
    // write clears it (char_len becomes 1) — a read placed after the write
    // always observes false and silently skips the cleanup.

    // AC-2 (task0004): a width-1 base cell can itself carry overflow
    // content (a long combining-mark run whose base has width 1) — this
    // proves the fast path's guard is gated on the cell's own marker, not
    // reused from `old_width != 1` (the neighbor-repair branch's
    // condition), which would miss this cell entirely since its width is
    // already 1.
    #[test]
    fn process_pty_data_ascii_overwrites_width1_overflow_cell_removes_overflow_entry() {
        let mut core = TerminalCore::new(20, 3, 0);
        let mut setup = vec![b'e'];
        for m in [
            0x0301u32, 0x0302, 0x0303, 0x0304, 0x0305, 0x0306, 0x0307, 0x0308,
        ] {
            let ch = char::from_u32(m).unwrap();
            let mut buf = [0u8; 4];
            setup.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
        core.process_pty_data(&setup); // setup, own call (non-ASCII bytes: slow path)

        // Pre assert: overflow-bound, but width stays 1.
        let idx0 = core.cell_index(0, 0).expect("col0 row0 in bounds");
        assert!(core.ring_cells[idx0].is_overflow());
        assert_eq!(core.get_cell_width(0, 0), 1);
        let abs = core.viewport_abs(0) as u32;
        assert!(core.overflow.contains_key(&(0u32, abs)));

        core.process_pty_data(b"\rZ"); // CR then 'Z': fast-path-eligible call

        assert_eq!(core.get_cell_char(0, 0), "Z");
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert!(!core.overflow.contains_key(&(0u32, abs)));
        assert!(!core.overflow_ridx.contains_key(&abs));
    }

    // AC-3 (task0004, FR3/FR7): with the overflow table non-empty because
    // of an UNRELATED cell (a different row), overwriting an ordinary
    // (non-overflow) cell via the dispatch fast path leaves the unrelated
    // entry and its reverse-index entry intact.
    #[test]
    fn process_pty_data_ascii_overwrites_ordinary_cell_leaves_unrelated_overflow_entry_intact() {
        let mut core = TerminalCore::new(10, 3, 0);
        core.process_pty_data(b"\x1b[2;1H"); // CUP row2,col1 (1-indexed) -> row1,col0
        core.process_pty_data(
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}".as_bytes(),
        );
        core.process_pty_data(b"\r"); // flush the buffered cluster
        let unrelated_abs = core.viewport_abs(1) as u32;
        assert!(core.overflow.contains_key(&(0u32, unrelated_abs)));

        core.process_pty_data(b"\x1b[1;1H"); // CUP row1,col1 (1-indexed) -> row0,col0
        core.process_pty_data(b"Z"); // fast-path-eligible call, non-overflow cell

        assert_eq!(core.get_cell_char(0, 0), "Z");
        assert_eq!(core.get_cell_width(0, 0), 1);
        assert!(core.overflow.contains_key(&(0u32, unrelated_abs)));
        assert!(
            core.overflow_ridx
                .get(&unrelated_abs)
                .map(|cols| cols.contains(&0u32))
                .unwrap_or(false)
        );
    }

    // AC-3 (task0004, FR3/FR7): parity between the dispatch fast path and
    // the print path's own ASCII writer for the AC-3 scenario above —
    // identical grid contents, widths, and overflow-table state.
    #[test]
    fn process_pty_data_ascii_overwrite_with_unrelated_overflow_matches_print_path_write() {
        let setup = |core: &mut TerminalCore| {
            core.process_pty_data(b"\x1b[2;1H"); // CUP -> row1,col0
            core.process_pty_data(
                "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}".as_bytes(),
            );
            core.process_pty_data(b"\r"); // flush the buffered cluster
            core.process_pty_data(b"\x1b[1;1H"); // CUP -> row0,col0
        };

        // Print path: the overwriting 'Z' is delivered through
        // handle_print_ascii via a direct call.
        let mut core_print_path = TerminalCore::new(10, 3, 0);
        setup(&mut core_print_path);
        core_print_path.handle_print(0x5A); // 'Z' via handle_print_ascii directly

        // Fast path: the overwriting 'Z' arrives in its own dispatch call
        // at a ground-clean chunk boundary, entering the inlined
        // fast-path writer.
        let mut core_fast_path = TerminalCore::new(10, 3, 0);
        setup(&mut core_fast_path);
        core_fast_path.process_pty_data(b"Z"); // fast-path-eligible call

        // The unrelated entry survived the overwrite (not just "both
        // writers agree" — both could agree while both being wrong).
        let unrelated_abs = core_fast_path.viewport_abs(1) as u32;
        assert!(core_fast_path.overflow.contains_key(&(0u32, unrelated_abs)));
        assert!(
            core_fast_path
                .overflow_ridx
                .get(&unrelated_abs)
                .map(|cols| cols.contains(&0u32))
                .unwrap_or(false)
        );

        for row in 0..3u16 {
            for col in 0..10u16 {
                assert_eq!(
                    core_fast_path.get_cell_char(col, row),
                    core_print_path.get_cell_char(col, row),
                    "row {row} col {col}: char mismatch"
                );
                assert_eq!(
                    core_fast_path.get_cell_width(col, row),
                    core_print_path.get_cell_width(col, row),
                    "row {row} col {col}: width mismatch"
                );
            }
        }
        assert_eq!(core_fast_path.overflow, core_print_path.overflow);
        assert_eq!(core_fast_path.overflow_ridx, core_print_path.overflow_ridx);
    }
}
