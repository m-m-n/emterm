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
            if idx == 0 { continue; } // index 0 is reserved (no hyperlink)
            if entry.is_some() && !live_ids.contains(&(idx as u16)) {
                *entry = None;
            }
        }
    }

    pub(crate) fn handle_osc_internal(&mut self, param: u16, data: &str) {
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
            0 => 0,       // SetTitleAndIcon
            1 => 1,       // SetIconName
            2 => 2,       // SetTitle
            4 => 4,       // SetColorPalette
            7 => 7,       // SetWorkingDirectory
            8 => 8,       // Hyperlink
            9 => 9,       // Notification
            10 => 10,     // SetForegroundColor
            11 => 11,     // SetBackgroundColor
            12 => 12,     // SetCursorColor
            22 => 22,     // CursorShape
            52 => 52,     // Clipboard
            104 => 104,   // ResetColorPalette
            110 => 110,   // ResetForegroundColor
            111 => 111,   // ResetBackgroundColor
            112 => 112,   // ResetCursorColor
            133 => 133,   // SemanticPrompt
            777 => 100,   // EmtermExtension (mapped to 100)
            1337 => 101,  // iTerm2 protocol (mapped to 101, >255)
            _ => 255,     // Unknown
        };

        self.fire_osc_callback(action_type, data);
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
}
