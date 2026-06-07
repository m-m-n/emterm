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

        // OSC 777;emterm;fold;begin|end: capture a custom-fold mark with the
        // absolute row it was emitted on, mirroring the OSC 133 capture above.
        // This is *in addition to* the callback below: the wasm/WebView path
        // still consumes `on_osc(777, …)` (status-bar dispatcher / legacy
        // viewer queue), while native consumers drain `take_fold_marks` and do
        // the begin↔end pairing themselves.
        //
        // Suppressed on the alternate screen for the same reason as OSC 133: a
        // full-screen app's fold marker would be stamped with a meaningless
        // row (primary scrollback + alt cursor). Mirrors the WebView
        // `handleFoldCommand`'s `isAlternateBuffer` early-return.
        if param == 777 && !self.get_mode(crate::terminal_core::MODE_ALT_SCREEN) {
            if let Some((kind, label)) = parse_emterm_fold_mark(data) {
                self.push_pending_fold_mark(kind, label);
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

/// Maximum byte length of a stored fold-mark label.
///
/// Display truncates at 80 chars anyway, so 256 bytes has no user-visible
/// effect. The cap defends against a remote process emitting arbitrarily large
/// OSC 777 payloads (up to MAX_OSC_LEN = 16 MB each) to cause a
/// denial-of-service via unbounded label allocations across up to 4096 pending
/// fold marks.
const MAX_FOLD_LABEL_BYTES: usize = 256;

/// Truncate `s` to at most `max_bytes` bytes, preserving UTF-8 char boundaries.
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Walk char boundaries to find the largest prefix that fits.
    let mut boundary = 0;
    for (idx, _) in s.char_indices() {
        if idx > max_bytes {
            break;
        }
        boundary = idx;
    }
    &s[..boundary]
}

/// Parse an OSC 777 payload into a custom-fold mark `(kind, label)`.
///
/// Forms (mirrors the WebView `handleFoldCommand`):
/// - `emterm;fold;begin` or `emterm;fold;begin;<label>` → [`FoldMarkKind::Begin`]
/// - `emterm;fold;end` → [`FoldMarkKind::End`]
///
/// The label is the remainder after `begin;` verbatim (which may itself
/// contain `;`), so `begin;a;b` yields label `"a;b"`. A missing label is the
/// empty string; the consumer applies the `"..."` fallback at registration,
/// matching the WebView `params[1] || "..."`. Any other payload (a different
/// emterm verb, an unknown fold sub-command, or a stray trailing field on
/// `end`) returns `None` so non-fold OSC 777 traffic is never captured here —
/// it still flows through the callback for the status-bar / viewer path.
///
/// Labels are truncated to [`MAX_FOLD_LABEL_BYTES`] bytes at a UTF-8 char
/// boundary before storage, preventing unbounded memory use from large OSC
/// payloads.
fn parse_emterm_fold_mark(data: &str) -> Option<(crate::terminal_core::FoldMarkKind, String)> {
    use crate::terminal_core::FoldMarkKind;
    let rest = data.strip_prefix("emterm;fold;")?;
    if let Some(label) = rest.strip_prefix("begin;") {
        let label = truncate_to_char_boundary(label, MAX_FOLD_LABEL_BYTES);
        Some((FoldMarkKind::Begin, label.to_string()))
    } else if rest == "begin" {
        Some((FoldMarkKind::Begin, String::new()))
    } else if rest == "end" {
        Some((FoldMarkKind::End, String::new()))
    } else {
        None
    }
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

    // ── OSC 777 emterm;fold pending marks ─────────────────────────────

    use crate::terminal_core::FoldMarkKind;

    #[test]
    fn test_parse_emterm_fold_mark_forms() {
        use super::parse_emterm_fold_mark;
        assert_eq!(
            parse_emterm_fold_mark("emterm;fold;begin"),
            Some((FoldMarkKind::Begin, String::new()))
        );
        assert_eq!(
            parse_emterm_fold_mark("emterm;fold;begin;Build Output"),
            Some((FoldMarkKind::Begin, "Build Output".to_string()))
        );
        // A label containing `;` is preserved verbatim.
        assert_eq!(
            parse_emterm_fold_mark("emterm;fold;begin;a;b"),
            Some((FoldMarkKind::Begin, "a;b".to_string()))
        );
        // An empty label after `begin;` stays empty (consumer applies "...").
        assert_eq!(
            parse_emterm_fold_mark("emterm;fold;begin;"),
            Some((FoldMarkKind::Begin, String::new()))
        );
        assert_eq!(
            parse_emterm_fold_mark("emterm;fold;end"),
            Some((FoldMarkKind::End, String::new()))
        );
        // Non-fold / unknown payloads are not captured.
        assert_eq!(parse_emterm_fold_mark("emterm;fold;toggle"), None);
        assert_eq!(parse_emterm_fold_mark("emterm;markdown;..."), None);
        assert_eq!(parse_emterm_fold_mark("emterm;fold;end;extra"), None);
        assert_eq!(parse_emterm_fold_mark(""), None);
    }

    #[test]
    fn test_parse_emterm_fold_mark_label_truncated_at_256_bytes() {
        use super::parse_emterm_fold_mark;
        // A label longer than MAX_FOLD_LABEL_BYTES must be silently truncated.
        let long_label = "x".repeat(512);
        let payload = format!("emterm;fold;begin;{long_label}");
        let result = parse_emterm_fold_mark(&payload);
        let (kind, label) = result.expect("should parse");
        assert_eq!(kind, FoldMarkKind::Begin);
        assert!(
            label.len() <= 256,
            "label must be <= 256 bytes, got {}",
            label.len()
        );
        // The preserved prefix must be the first 256 bytes of the original.
        assert_eq!(label, &long_label[..256]);
    }

    #[test]
    fn test_parse_emterm_fold_mark_label_truncated_at_char_boundary() {
        use super::parse_emterm_fold_mark;
        // Each '日' is 3 bytes. 85 × 3 = 255 bytes, plus one more '日' would
        // be 258 bytes — over the 256-byte cap. The truncation must land on
        // the char boundary before byte 256, yielding exactly 85 chars / 255
        // bytes, not splitting the 86th '日' mid-sequence.
        let multibyte_label = "日".repeat(100); // 300 bytes total
        let payload = format!("emterm;fold;begin;{multibyte_label}");
        let result = parse_emterm_fold_mark(&payload);
        let (kind, label) = result.expect("should parse");
        assert_eq!(kind, FoldMarkKind::Begin);
        // Must be valid UTF-8 (would panic on to_string() if not, but assert
        // explicitly for clarity).
        assert!(std::str::from_utf8(label.as_bytes()).is_ok());
        assert!(
            label.len() <= 256,
            "label must be <= 256 bytes, got {}",
            label.len()
        );
        // 255 bytes = 85 × '日' is the largest multiple of 3 that fits in 256.
        assert_eq!(label.len(), 255);
        assert_eq!(label.chars().count(), 85);
    }

    #[test]
    fn test_fold_begin_records_current_row_and_label() {
        let mut core = TerminalCore::new(80, 24, 1000);
        // Advance the cursor two rows, then emit a begin with a label.
        core.process_pty_data(b"\r\n\r\n\x1b]777;emterm;fold;begin;hello\x07");
        let marks = core.take_fold_marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].kind, FoldMarkKind::Begin);
        assert_eq!(marks[0].abs_row, 2);
        assert_eq!(marks[0].label, "hello");
        assert_eq!(marks[0].evicted_total, 0);
    }

    #[test]
    fn test_fold_marks_keep_distinct_rows_in_one_chunk() {
        // Like the OSC 133 multi-mark test: a begin and end separated by
        // newlines in one chunk must land on the rows they were emitted on.
        let mut core = TerminalCore::new(80, 24, 1000);
        let chunk =
            b"\x1b]777;emterm;fold;begin;lbl\x07line0\r\nline1\r\n\x1b]777;emterm;fold;end\x07";
        core.process_pty_data(chunk);
        let marks = core.take_fold_marks();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].kind, FoldMarkKind::Begin);
        assert_eq!(marks[0].abs_row, 0);
        assert_eq!(marks[0].label, "lbl");
        assert_eq!(marks[1].kind, FoldMarkKind::End);
        assert_eq!(marks[1].abs_row, 2);
        assert_eq!(marks[1].label, "");
    }

    #[test]
    fn test_fold_marks_record_eviction_snapshot() {
        let mut core = TerminalCore::new(80, 2, 2);
        core.process_pty_data(b"\x1b]777;emterm;fold;begin;a\x07");
        let first_evicted = {
            let m = core.take_fold_marks();
            assert_eq!(m.len(), 1);
            m[0].evicted_total
        };
        for _ in 0..10 {
            core.process_pty_data(b"x\r\n");
        }
        core.process_pty_data(b"\x1b]777;emterm;fold;end\x07");
        let marks = core.take_fold_marks();
        assert_eq!(marks.len(), 1);
        assert!(
            marks[0].evicted_total >= first_evicted,
            "second mark eviction snapshot {} should be >= first {}",
            marks[0].evicted_total,
            first_evicted
        );
        assert!(core.get_scrollback_evicted_total() > 0, "expected eviction");
    }

    #[test]
    fn test_fold_marks_suppressed_on_alt_screen() {
        // Mirrors the OSC 133 alt-screen suppression and the WebView
        // `handleFoldCommand` `isAlternateBuffer` guard.
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data_fully(
            b"\x1b[?1049h\x1b]777;emterm;fold;begin;x\x07\x1b[?1049l\x1b]777;emterm;fold;begin;y\x07",
        );
        let marks = core.take_fold_marks();
        assert_eq!(marks.len(), 1, "only the primary-screen mark survives");
        assert_eq!(marks[0].label, "y");
    }

    #[test]
    fn test_fold_unknown_payload_not_recorded() {
        let mut core = TerminalCore::new(80, 24, 1000);
        // A non-fold OSC 777 (e.g. a viewer trigger) must not produce a mark.
        core.process_pty_data(b"\x1b]777;emterm;fold;toggle\x07");
        assert!(core.take_fold_marks().is_empty());
    }

    #[test]
    fn test_pending_fold_marks_capped() {
        let mut core = TerminalCore::new(80, 24, 1000);
        // Flood begin without advancing the cursor. The buffer must stay
        // bounded at MAX_PENDING_FOLD_MARKS.
        let one = b"\x1b]777;emterm;fold;begin;x\x07";
        let n = crate::terminal_core::MAX_PENDING_FOLD_MARKS + 100;
        let mut flood = Vec::with_capacity(one.len() * n);
        for _ in 0..n {
            flood.extend_from_slice(one);
        }
        core.process_pty_data(&flood);
        let marks = core.take_fold_marks();
        assert_eq!(marks.len(), crate::terminal_core::MAX_PENDING_FOLD_MARKS);
    }

    #[test]
    fn test_take_fold_marks_clears_buffer() {
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data(b"\x1b]777;emterm;fold;begin;a\x07");
        assert_eq!(core.take_fold_marks().len(), 1);
        assert!(core.take_fold_marks().is_empty());
    }

    #[test]
    fn test_reset_clears_pending_fold_marks() {
        let mut core = TerminalCore::new(80, 24, 1000);
        core.process_pty_data(b"\x1b]777;emterm;fold;begin;a\x07");
        core.reset();
        assert!(core.take_fold_marks().is_empty());
    }
}
