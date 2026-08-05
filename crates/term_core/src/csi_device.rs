/// CSI device response handlers: DSR, DA1, DA2, response buffer.
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    /// CSI Ps n - Device Status Report.
    /// Returns response length (0 if no response).
    pub fn handle_device_status_report(&mut self, ps: u8) -> u8 {
        match ps {
            5 => {
                // OK status
                self.write_response(b"\x1b[0n")
            }
            6 => {
                // Cursor position report (1-indexed)
                self.format_cpr()
            }
            _ => 0, // Unknown: no response
        }
    }

    /// CSI c - Primary Device Attributes.
    /// Returns response length.
    pub fn handle_primary_device_attributes(&mut self) -> u8 {
        self.write_response(b"\x1b[?65;1;4;22c")
    }

    /// CSI > c - Secondary Device Attributes.
    /// Returns response length.
    pub fn handle_secondary_device_attributes(&mut self) -> u8 {
        self.write_response(b"\x1b[>65;1;0c")
    }

    /// Total bytes currently pending in the ordered response store
    /// (task0002 D5, review-round-1 rework): the SUM of every response
    /// synthesized since the previous [`Self::take_response`] drain, not
    /// merely the most recently synthesized single response. Zero means
    /// nothing is pending.
    pub fn get_response_len(&self) -> u32 {
        self.response_queue.len() as u32
    }

    /// Peek at the ordered pending-response store WITHOUT draining it
    /// (task0002 D5): returns every response synthesized since the
    /// previous [`Self::take_response`] drain, concatenated in synthesis
    /// order. Non-destructive counterpart to `take_response` — a caller
    /// that needs the exactly-once PTY delivery guarantee must drain via
    /// `take_response`, not read this.
    pub fn get_response_bytes(&self) -> Vec<u8> {
        self.response_queue.clone()
    }

    /// Drain and return every pending device response, concatenated in
    /// synthesis order, clearing the store (task0002 D5: a drain whose
    /// result is discarded — e.g. the snapshot/replay paths — removes
    /// everything pending). A second call immediately after returns empty.
    ///
    /// The native-poc embedder calls this after every `process_pty_data_fully`
    /// to forward DSR / DA / XTWINOPS replies back into the PTY's input side.
    /// Without it PSReadLine (PowerShell, Windows) issues `\x1b[6n` cursor-
    /// position queries during line redraws, never receives an answer, and
    /// recomputes the redraw against a stale cursor — manifesting as
    /// backspace erasing far more characters than the one the user pressed.
    ///
    /// This is the SOLE PTY delivery route for a synthesized response
    /// (tmux-startup-query-response-leak task0001 / task0002): the
    /// embedder's three write-back sites poll this after every parse and
    /// are the only callers that may forward the bytes onward. `term_core`
    /// fires no competing callback for device responses — a second live
    /// consumer would reintroduce exactly-once-delivery violations (a
    /// query's application receiving its own reply more than once, the
    /// original task0001 bug).
    pub fn take_response(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.response_queue)
    }
}

impl TerminalCore {
    /// CSI ? Ps $ p - DECRPM (DEC Private Mode Report).
    /// Reports whether a DEC private mode is set, reset, or not recognized.
    /// Response: CSI ? Ps ; Pm $ y
    ///   Pm=0: not recognized, Pm=1: set, Pm=2: reset
    pub fn handle_decrpm(&mut self, mode: u16) -> u8 {
        let pm: u8 = match mode {
            // Known modes tracked in WASM bitfield
            3 => {
                if self.get_mode(crate::terminal_core::MODE_COLUMN_132) {
                    1
                } else {
                    2
                }
            }
            5 => {
                if self.get_mode(crate::terminal_core::MODE_REVERSE_SCREEN) {
                    1
                } else {
                    2
                }
            }
            6 => {
                if self.get_mode(crate::terminal_core::MODE_ORIGIN) {
                    1
                } else {
                    2
                }
            }
            7 => {
                if self.get_mode(crate::terminal_core::MODE_AUTO_WRAP) {
                    1
                } else {
                    2
                }
            }
            12 => {
                if self.get_mode(crate::terminal_core::MODE_CURSOR_BLINK) {
                    1
                } else {
                    2
                }
            }
            25 => {
                if self.get_mode(crate::terminal_core::MODE_CURSOR_VISIBLE) {
                    1
                } else {
                    2
                }
            }
            1004 => {
                if self.get_mode(crate::terminal_core::MODE_FOCUS_TRACKING) {
                    1
                } else {
                    2
                }
            }
            2004 => {
                if self.get_mode(crate::terminal_core::MODE_BRACKETED_PASTE) {
                    1
                } else {
                    2
                }
            }
            2026 => {
                if self.get_mode(crate::terminal_core::MODE_SYNCHRONIZED_OUTPUT) {
                    1
                } else {
                    2
                }
            }
            // Known modes tracked in TS (report as recognized but defer to TS)
            1 | 47 | 1000 | 1002 | 1003 | 1005 | 1006 | 1047 | 1048 | 1049 => 2,
            // Unknown modes
            _ => 0,
        };
        // Format: ESC [ ? <mode> ; <pm> $ y
        let mut buf = [0u8; 20];
        buf[0] = b'\x1b';
        buf[1] = b'[';
        buf[2] = b'?';
        let mut pos = 3;
        pos = Self::write_u16_decimal(&mut buf, pos, mode);
        buf[pos] = b';';
        pos += 1;
        buf[pos] = pm + b'0';
        pos += 1;
        buf[pos] = b'$';
        pos += 1;
        buf[pos] = b'y';
        pos += 1;
        self.write_response(&buf[..pos])
    }

    /// Append bytes to the ordered pending-response store (task0002 D5:
    /// APPENDS, never overwrites — see [`TerminalCore::response_queue`]).
    /// Returns the number of bytes just appended by THIS call (the
    /// individual response's length), not the store's new total —
    /// matching the per-call contract every `handle_*` caller and its
    /// tests rely on.
    fn write_response(&mut self, data: &[u8]) -> u8 {
        self.response_queue.extend_from_slice(data);
        data.len().min(u8::MAX as usize) as u8
    }

    /// Format cursor position report into response buffer.
    fn format_cpr(&mut self) -> u8 {
        let row = self.cursor.row.saturating_add(1);
        let col = self.cursor.col.saturating_add(1);
        // Format: ESC [ row ; col R
        let mut buf = [0u8; 20];
        buf[0] = b'\x1b';
        buf[1] = b'[';
        let mut pos = 2;
        pos = Self::write_u16_decimal(&mut buf, pos, row);
        buf[pos] = b';';
        pos += 1;
        pos = Self::write_u16_decimal(&mut buf, pos, col);
        buf[pos] = b'R';
        pos += 1;
        self.write_response(&buf[..pos])
    }

    /// CSI 14 t - Report text area size in pixels.
    /// Response: ESC [ 4 ; <height> ; <width> t
    pub fn handle_xtwinops_text_area_px(&mut self) -> u8 {
        let height = self.rows as u32 * self.cell_height_px as u32;
        let width = self.cols as u32 * self.cell_width_px as u32;
        self.format_xtwinops(4, height, width)
    }

    /// CSI 16 t - Report character cell size in pixels.
    /// Response: ESC [ 6 ; <height> ; <width> t
    pub fn handle_xtwinops_cell_size(&mut self) -> u8 {
        self.format_xtwinops(6, self.cell_height_px as u32, self.cell_width_px as u32)
    }

    /// CSI 18 t - Report text area size in characters.
    /// Response: ESC [ 8 ; <rows> ; <cols> t
    pub fn handle_xtwinops_text_area_chars(&mut self) -> u8 {
        self.format_xtwinops(8, self.rows as u32, self.cols as u32)
    }

    /// Format XTWINOPS response: ESC [ <ps> ; <p1> ; <p2> t
    fn format_xtwinops(&mut self, ps: u8, p1: u32, p2: u32) -> u8 {
        let mut buf = [0u8; 32];
        buf[0] = b'\x1b';
        buf[1] = b'[';
        let mut pos = 2;
        buf[pos] = ps + b'0';
        pos += 1;
        buf[pos] = b';';
        pos += 1;
        pos = Self::write_u32_decimal(&mut buf, pos, p1);
        buf[pos] = b';';
        pos += 1;
        pos = Self::write_u32_decimal(&mut buf, pos, p2);
        buf[pos] = b't';
        pos += 1;
        self.write_response(&buf[..pos])
    }

    /// Write a u16 as decimal digits to buffer, return new position.
    fn write_u16_decimal(buf: &mut [u8], start: usize, val: u16) -> usize {
        if val == 0 {
            buf[start] = b'0';
            return start + 1;
        }
        let mut digits = [0u8; 5];
        let mut n = val;
        let mut count = 0;
        while n > 0 {
            digits[count] = (n % 10) as u8 + b'0';
            n /= 10;
            count += 1;
        }
        let mut pos = start;
        for i in (0..count).rev() {
            buf[pos] = digits[i];
            pos += 1;
        }
        pos
    }

    /// Write a u32 as decimal digits to buffer, return new position.
    fn write_u32_decimal(buf: &mut [u8], start: usize, val: u32) -> usize {
        if val == 0 {
            buf[start] = b'0';
            return start + 1;
        }
        let mut digits = [0u8; 10];
        let mut n = val;
        let mut count = 0;
        while n > 0 {
            digits[count] = (n % 10) as u8 + b'0';
            n /= 10;
            count += 1;
        }
        let mut pos = start;
        for i in (0..count).rev() {
            buf[pos] = digits[i];
            pos += 1;
        }
        pos
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    // ── Sprint 4: Device Response Tests ─────────────────────

    #[test]
    fn test_dsr_ok_status() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_device_status_report(5);
        assert_eq!(len, 4);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[0n");
    }

    #[test]
    fn test_dsr_cursor_position_home() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(0, 0);
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[1;1R");
    }

    #[test]
    fn test_dsr_cursor_position_nonzero() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cursor(9, 4); // 0-indexed
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[5;10R"); // 1-indexed
    }

    #[test]
    fn test_dsr_unknown() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_device_status_report(99);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_da1() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_primary_device_attributes();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?65;1;4;22c");
    }

    #[test]
    fn test_da2() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_secondary_device_attributes();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[>65;1;0c");
    }

    /// `get_response_len` under queue semantics (task0002 D5): total
    /// pending bytes, not "did the last call produce a response".
    /// `get_response_ptr` (the raw-pointer WebView-era accessor this test
    /// used to also exercise) is removed — task0002 audit found no
    /// production caller (the crate deliberately ships no wasm-bindgen
    /// surface), and a raw pointer into a store that can now grow/
    /// reallocate on every subsequent query is a latent hazard under the
    /// new append-only representation.
    #[test]
    fn test_response_len_reflects_pending_queue() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_primary_device_attributes();
        let len = core.get_response_len();
        assert!(len > 0);
    }

    #[test]
    fn test_dsr_large_position() {
        let mut core = TerminalCore::new(500, 500, 0);
        core.set_cursor(499, 499);
        let len = core.handle_device_status_report(6);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[500;500R");
    }

    // ── XTWINOPS Tests ──────────────────────────────────

    #[test]
    fn test_xtwinops_cell_size() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_size_px(10, 20);
        let len = core.handle_xtwinops_cell_size();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;20;10t");
    }

    #[test]
    fn test_xtwinops_text_area_px() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.set_cell_size_px(10, 20);
        let len = core.handle_xtwinops_text_area_px();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        // 24 rows * 20px = 480, 80 cols * 10px = 800
        assert_eq!(&bytes, b"\x1b[4;480;800t");
    }

    #[test]
    fn test_xtwinops_text_area_chars() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_xtwinops_text_area_chars();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[8;24;80t");
    }

    #[test]
    fn test_xtwinops_default_cell_size() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Default: 8x16
        let len = core.handle_xtwinops_cell_size();
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[6;16;8t");
    }

    // ── DECRPM Tests ──────────────────────────────────────

    #[test]
    fn test_decrpm_mode_2026_reset() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_decrpm(2026);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?2026;2$y"); // Pm=2 (reset)
    }

    #[test]
    fn test_decrpm_mode_2026_set() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.handle_set_mode(2026, true);
        let len = core.handle_decrpm(2026);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?2026;1$y"); // Pm=1 (set)
    }

    #[test]
    fn test_decrpm_known_mode_autowrap() {
        let mut core = TerminalCore::new(80, 24, 0);
        // autoWrap defaults to true
        let len = core.handle_decrpm(7);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?7;1$y"); // Pm=1 (set)
    }

    #[test]
    fn test_decrpm_unknown_mode() {
        let mut core = TerminalCore::new(80, 24, 0);
        let len = core.handle_decrpm(9999);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?9999;0$y"); // Pm=0 (not recognized)
    }

    #[test]
    fn test_decrpm_ts_tracked_mode() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Mode 1 (DECCKM) is tracked in TS, reported as reset (2)
        let len = core.handle_decrpm(1);
        assert!(len > 0);
        let bytes = core.get_response_bytes();
        assert_eq!(&bytes, b"\x1b[?1;2$y"); // Pm=2 (reset)
    }

    #[test]
    fn test_cell_size_getters() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Default values
        assert_eq!(core.get_cell_width_px(), 8);
        assert_eq!(core.get_cell_height_px(), 16);

        // After setting
        core.set_cell_size_px(10, 20);
        assert_eq!(core.get_cell_width_px(), 10);
        assert_eq!(core.get_cell_height_px(), 20);
    }

    // ── Ordered multi-response drain (tmux-startup-query-response-leak
    // task0002, review-round-1 rework, D5) ──────────────────────────────
    //
    // AC-1: five (here seven) distinct in-scope queries fed in ONE parse
    // call must all survive a single subsequent drain, concatenated in
    // synthesis order, with a second drain returning empty. Pre-fix, the
    // single-slot `response_buffer` is overwritten by each `write_response`
    // call, so only the LAST query's reply (XTWINOPS 18) survives — this
    // test fails against that code (orchestrator-verified byte-level
    // probe: `TerminalCore::new(80, 24, 1000)` then
    // `process_pty_data_fully(b"\x1b[c\x1b[>0c\x1b[14t\x1b[16t\x1b[18t")`
    // yields only `ESC[8;24;80t` from `take_response`).
    #[test]
    fn take_response_after_multi_query_chunk_returns_all_in_order() {
        let mut core = TerminalCore::new(80, 24, 0);
        // DA1, DA2, DSR status, CPR, XTWINOPS 14/16/18 in ONE parse call.
        core.process_pty_data_fully(b"\x1b[c\x1b[>0c\x1b[5n\x1b[6n\x1b[14t\x1b[16t\x1b[18t");

        let drained = core.take_response();
        let expected: Vec<u8> = [
            &b"\x1b[?65;1;4;22c"[..], // DA1
            &b"\x1b[>65;1;0c"[..],    // DA2
            &b"\x1b[0n"[..],          // DSR status
            &b"\x1b[1;1R"[..],        // CPR
            &b"\x1b[4;384;640t"[..],  // XTWINOPS 14
            &b"\x1b[6;16;8t"[..],     // XTWINOPS 16
            &b"\x1b[8;24;80t"[..],    // XTWINOPS 18
        ]
        .concat();
        assert_eq!(
            drained, expected,
            "a single drain after a multi-query chunk must return every \
             response, concatenated in synthesis order"
        );

        assert!(
            core.take_response().is_empty(),
            "a second drain immediately after must return empty"
        );
    }
}
