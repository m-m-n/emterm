/// OSC internal dispatch: routes ParsedAction::OscDispatch to callbacks.
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    /// Allocate a hyperlink ID and store the entry in the hyperlink table.
    fn allocate_hyperlink(&mut self, params: &str, uri: &str) -> u16 {
        // Run GC when table grows large to reclaim unused entries
        if self.hyperlink_table.len() > 1024 {
            self.gc_hyperlink_table();
        }
        let id = self.hyperlink_next_id;
        // Ensure table is large enough
        while self.hyperlink_table.len() <= id as usize {
            self.hyperlink_table.push(None);
        }
        self.hyperlink_table[id as usize] = Some((params.to_string(), uri.to_string()));
        // Advance ID, wrapping around but skipping 0
        self.hyperlink_next_id = if id == u16::MAX { 1 } else { id + 1 };
        id
    }

    /// Garbage-collect the hyperlink table by scanning all cells for live IDs.
    fn gc_hyperlink_table(&mut self) {
        use std::collections::HashSet;
        let mut live_ids = HashSet::new();

        // Include the active hyperlink if set
        if self.active_hyperlink_id != 0 {
            live_ids.insert(self.active_hyperlink_id);
        }

        // Scan all cells in the ring buffer for live hyperlink IDs
        for cell in &self.ring_cells {
            if cell.hyperlink_id != 0 {
                live_ids.insert(cell.hyperlink_id);
            }
        }

        // Clear entries not referenced by any cell
        for (idx, entry) in self.hyperlink_table.iter_mut().enumerate() {
            if idx == 0 {
                continue;
            } // index 0 is reserved (no hyperlink)
            if entry.is_some() && !live_ids.contains(&(idx as u16)) {
                *entry = None;
            }
        }
    }

    pub(crate) fn handle_osc_internal(&mut self, param: u16, data: &str) {
        // OSC 9999: emterm mux message — route to APC callback for mux handling.
        // This allows mux to work through Windows ConPTY which strips APC but passes OSC.
        if param == 9999 {
            if data.starts_with("emterm-mux;") {
                self.fire_apc_callback(data.as_bytes());
            }
            return;
        }

        // Special handling for OSC 8: process hyperlink inline
        if param == 8 {
            if let Some(sep) = data.find(';') {
                let params = &data[..sep];
                let uri = &data[sep + 1..];
                if uri.is_empty() {
                    // Close hyperlink
                    self.active_hyperlink_id = 0;
                } else {
                    // Open hyperlink: allocate ID
                    let id = self.allocate_hyperlink(params, uri);
                    self.active_hyperlink_id = id;
                }
            }
            // Still fire callback to TS for metadata mirroring
            self.fire_osc_callback(8, data);
            return;
        }

        let action_type: u8 = match param {
            0 => 0,      // SetTitleAndIcon
            1 => 1,      // SetIconName
            2 => 2,      // SetTitle
            4 => 4,      // SetColorPalette
            7 => 7,      // SetWorkingDirectory
            8 => 8,      // Hyperlink
            9 => 9,      // Notification
            10 => 10,    // SetForegroundColor
            11 => 11,    // SetBackgroundColor
            12 => 12,    // SetCursorColor
            22 => 22,    // CursorShape
            52 => 52,    // Clipboard
            104 => 104,  // ResetColorPalette
            110 => 110,  // ResetForegroundColor
            111 => 111,  // ResetBackgroundColor
            112 => 112,  // ResetCursorColor
            133 => 133,  // SemanticPrompt
            777 => 100,  // EmtermExtension (mapped to 100)
            1337 => 101, // iTerm2 protocol (mapped to 101, >255)
            _ => 255,    // Unknown
        };

        // OSC 133 semantic prompt: capture the mark with the absolute row it
        // was emitted on *before* the rest of the chunk advances the cursor.
        // This is in addition to (not a replacement for) the callback below:
        // the wasm/WebView path consumes `on_osc(133, …)`, while native
        // consumers drain `take_prompt_marks` so multiple marks in one chunk
        // keep distinct rows.
        //
        // Suppressed on the alternate screen: a full-screen app's OSC 133
        // would be stamped with a meaningless row (primary scrollback +
        // alt cursor) and pollute prompt navigation after the app exits.
        // Mirrors the WebView's `isAlternateBuffer` guard and the
        // semantic-scroll-and-search SPEC edge case ("Alternate buffer
        // active: OSC 133 markers not recorded").
        if param == 133 && !self.get_mode(crate::terminal_core::MODE_ALT_SCREEN) {
            if let Some((kind, exit_code)) = parse_osc133_mark(data) {
                self.push_pending_prompt_mark(kind, exit_code);
            }
        }

        self.fire_osc_callback(action_type, data);
    }
}

/// Parse an OSC 133 payload into `(kind_byte, exit_code)`.
///
/// Forms (subset of the FinalTerm/iTerm2 semantic-prompt spec):
/// - `A` → prompt start
/// - `B` → command start
/// - `C` → command exec
/// - `D` or `D;<n>` → command end (optional exit code)
///
/// Any other head byte returns `None` so unknown marks are never recorded.
fn parse_osc133_mark(data: &str) -> Option<(u8, Option<i32>)> {
    let mut it = data.split(';');
    let head = it.next().unwrap_or("");
    let kind = match head {
        "A" => b'A',
        "B" => b'B',
        "C" => b'C',
        "D" => b'D',
        _ => return None,
    };
    let exit_code = if kind == b'D' {
        it.next().and_then(|s| s.parse::<i32>().ok())
    } else {
        None
    };
    Some((kind, exit_code))
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

    #[test]
    fn test_osc8_hyperlink_sets_cell_hyperlink_id() {
        let mut core = TerminalCore::new(80, 24, 1000);
        // OSC 8 open: \x1b]8;;http://example.com\x07
        // Then print "Hi"
        // Then OSC 8 close: \x1b]8;;\x07
        // All in one chunk (realistic scenario)
        let data = b"\x1b]8;;http://example.com\x07Hi\x1b]8;;\x07there";
        core.process_pty_data(data);

        // "H" at col 0, "i" at col 1 should have hyperlink_id > 0
        let hl0 = core.get_cell_hyperlink_id(0, 0);
        let hl1 = core.get_cell_hyperlink_id(1, 0);
        // "t" at col 2 should have hyperlink_id == 0
        let hl2 = core.get_cell_hyperlink_id(2, 0);
        assert!(hl0 > 0, "H should have hyperlink");
        assert!(hl1 > 0, "i should have hyperlink");
        assert_eq!(hl0, hl1, "same hyperlink ID");
        assert_eq!(hl2, 0, "t should not have hyperlink");

        // Verify URI
        let uri = core.get_hyperlink_uri(hl0);
        assert_eq!(uri, "http://example.com");
    }

    // ── OSC 133 pending prompt marks ──────────────────────────────────

    #[test]
    fn test_parse_osc133_mark_kinds() {
        assert_eq!(super::parse_osc133_mark("A"), Some((b'A', None)));
        assert_eq!(super::parse_osc133_mark("B"), Some((b'B', None)));
        assert_eq!(super::parse_osc133_mark("C"), Some((b'C', None)));
        assert_eq!(super::parse_osc133_mark("D"), Some((b'D', None)));
        assert_eq!(super::parse_osc133_mark("D;0"), Some((b'D', Some(0))));
        assert_eq!(super::parse_osc133_mark("D;42"), Some((b'D', Some(42))));
        assert_eq!(super::parse_osc133_mark("Z"), None);
        assert_eq!(super::parse_osc133_mark(""), None);
    }

    #[test]
    fn test_osc133_single_mark_records_current_row() {
        let mut core = TerminalCore::new(80, 24, 1000);
        // Move the cursor down two rows, then emit an A mark.
        core.process_pty_data(b"\r\n\r\n\x1b]133;A\x07");
        let marks = core.take_prompt_marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, b'A');
        // No scrollback yet, cursor on row 2.
        assert_eq!(marks[0].abs_row, 2);
        assert_eq!(marks[0].evicted_total, 0);
    }

    #[test]
    fn test_osc133_multiple_marks_in_one_chunk_keep_distinct_rows() {
        // The regression this fix targets: several OSC 133 marks separated by
        // newlines in a single chunk must NOT collapse onto the final cursor
        // row. Each take entry must carry the row it was emitted on.
        let mut core = TerminalCore::new(80, 24, 1000);
        let chunk = b"\x1b]133;A\x07line0\r\n\x1b]133;A\x07line1\r\n\x1b]133;A\x07line2";
        core.process_pty_data(chunk);
        let marks = core.take_prompt_marks();
        assert_eq!(marks.len(), 3);
        assert_eq!(marks[0].abs_row, 0);
        assert_eq!(marks[1].abs_row, 1);
        assert_eq!(marks[2].abs_row, 2);
        assert!(marks.iter().all(|m| m.kind == b'A'));
    }

    #[test]
    fn test_osc133_marks_record_eviction_snapshot() {
        // A small scrollback so we can force eviction between marks.
        // rows=2, scrollback=2 → total capacity 4 logical lines.
        let mut core = TerminalCore::new(80, 2, 2);
        // Emit a mark, then print enough newlines that the first mark's row
        // gets evicted out of scrollback. Each mark records the eviction
        // total *at emit time*.
        core.process_pty_data(b"\x1b]133;A\x07");
        let first_evicted = {
            let m = core.take_prompt_marks();
            assert_eq!(m.len(), 1);
            m[0].evicted_total
        };
        // Push many lines to evict rows, then a second mark.
        for _ in 0..10 {
            core.process_pty_data(b"x\r\n");
        }
        core.process_pty_data(b"\x1b]133;A\x07");
        let marks = core.take_prompt_marks();
        assert_eq!(marks.len(), 1);
        // The second mark sees a higher eviction total than the first.
        assert!(
            marks[0].evicted_total >= first_evicted,
            "second mark eviction snapshot {} should be >= first {}",
            marks[0].evicted_total,
            first_evicted
        );
        assert!(
            core.get_scrollback_evicted_total() > 0,
            "expected some eviction"
        );
    }

    #[test]
    fn test_osc133_unknown_kind_not_recorded() {
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data(b"\x1b]133;Z\x07");
        assert!(core.take_prompt_marks().is_empty());
    }

    #[test]
    fn test_osc133_suppressed_on_alt_screen() {
        // SPEC edge case ("Alternate buffer active: OSC 133 markers not
        // recorded") + WebView `isAlternateBuffer` guard: a full-screen app
        // emitting OSC 133 must not produce navigation marks.
        let mut core = TerminalCore::new(80, 24, 1000);
        // Enter alt screen (CSI ?1049h), emit a mark, leave (CSI ?1049l),
        // emit another. Only the post-exit mark must be recorded — the
        // switch and the marks live in ONE chunk (the resume loop carries
        // parsing across the buffer-switch interrupts), so this also pins
        // the parse-time (not chunk-level) granularity of the gate.
        core.process_pty_data_fully(b"\x1b[?1049h\x1b]133;A\x07\x1b[?1049l\x1b]133;A\x07");
        let marks = core.take_prompt_marks();
        assert_eq!(marks.len(), 1, "only the primary-screen mark survives");
        assert_eq!(marks[0].kind, b'A');
    }

    #[test]
    fn test_osc133_suppressed_on_alt_screen_mode_47() {
        // Legacy buffer-switch modes (?47 / ?1047) must gate the same way.
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data_fully(b"\x1b[?47h\x1b]133;A\x07");
        assert!(core.take_prompt_marks().is_empty());
        core.process_pty_data_fully(b"\x1b[?47l\x1b]133;A\x07");
        assert_eq!(core.take_prompt_marks().len(), 1);
    }

    #[test]
    fn test_process_pty_data_fully_resumes_after_buffer_switch() {
        // Regression for the data-loss bug the alt-gate tests exposed:
        // `process_pty_data` interrupts on a buffer switch and returns the
        // consumed byte count; a caller that ignores it drops the rest of
        // the chunk. The resume loop must process the remainder (here the
        // text after `?1049l`) and hand back the buffer-switch actions.
        let mut core = TerminalCore::new(80, 24, 1000);
        let actions = core.process_pty_data_fully(b"\x1b[?1049l\x1b]133;A\x07hello");
        assert_eq!(core.take_prompt_marks().len(), 1, "OSC after switch kept");
        assert_eq!(core.get_cell_char(0, 0), "h", "text after switch kept");
        assert!(actions.contains(&3), "SWITCH_TO_MAIN action surfaced");
    }

    #[test]
    fn test_osc133_d_exit_code_recorded() {
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data(b"\x1b]133;D;7\x07");
        let marks = core.take_prompt_marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, b'D');
        assert_eq!(marks[0].exit_code, Some(7));
    }

    #[test]
    fn test_pending_prompt_marks_capped() {
        let mut core = TerminalCore::new(80, 24, 1000);
        // Flood OSC 133 A without ever advancing past the cap. The buffer
        // must stay bounded at MAX_PENDING_PROMPT_MARKS.
        let one = b"\x1b]133;A\x07";
        let n = crate::terminal_core::MAX_PENDING_PROMPT_MARKS + 100;
        let mut flood = Vec::with_capacity(one.len() * n);
        for _ in 0..n {
            flood.extend_from_slice(one);
        }
        core.process_pty_data(&flood);
        let marks = core.take_prompt_marks();
        assert_eq!(marks.len(), crate::terminal_core::MAX_PENDING_PROMPT_MARKS);
    }

    #[test]
    fn test_take_prompt_marks_clears_buffer() {
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data(b"\x1b]133;A\x07");
        assert_eq!(core.take_prompt_marks().len(), 1);
        // Second take is empty — the first drained it.
        assert!(core.take_prompt_marks().is_empty());
    }

    #[test]
    fn test_reset_clears_pending_prompt_marks() {
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data(b"\x1b]133;A\x07");
        core.reset();
        assert!(core.take_prompt_marks().is_empty());
    }

}
