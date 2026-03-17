/// PTY data dispatch: process_pty_data, dispatch_action, and buffer switch detection.
use wasm_bindgen::prelude::*;

use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
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
        // When data contains far more newlines than the ring capacity,
        // skip processing lines that will be evicted from scrollback anyway.
        // This reduces 10M lines of `seq` to ~10K lines of actual processing.
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

            if all_fast && newline_count > self.ring_capacity {
                // Find the byte position of the (newline_count - ring_capacity)th newline
                let skip_lines = newline_count - self.ring_capacity;
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
                                let cell = &mut self.ring_cells[idx];
                                cell.char_data[0] = b;
                                cell.char_len = 1;
                                cell.width = 1;
                                cell.fg = self.cursor.fg;
                                cell.bg = self.cursor.bg;
                                cell.flags = self.cursor.flags;
                                cell.hyperlink_id = self.active_hyperlink_id;
                                self.mark_row_dirty(self.cursor.row);
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
                            pos += 1;
                        }
                        // HT: horizontal tab
                        0x09 => {
                            self.cursor.col = self.next_tab_stop(self.cursor.col);
                            self.wrap_pending = false;
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
            ParsedAction::Execute(byte) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
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
                self.handle_esc_internal(intermediate, final_byte);
            }
            ParsedAction::OscDispatch { param, data } => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
                self.handle_osc_internal(param, &data);
            }
            ParsedAction::ApcDispatch(payload) => {
                if !self.grapheme_buffer.is_empty() {
                    self.flush_grapheme_buffer();
                }
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
                self.fire_dcs_callback(&payload);
            }
        }
    }
}
