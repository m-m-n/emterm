/// Dedicated snapshot type for terminal state serialization.
///
/// Separated from the runtime `TerminalCore` to cleanly exclude
/// non-serializable fields (callbacks, pixel metrics) and to provide
/// a versioned envelope for forward compatibility.
use std::collections::HashMap;

use bincode::Options;
use serde::{Deserialize, Serialize};

use crate::cell::Cell;
use crate::terminal_core::{CursorState, TerminalCore};

/// Current snapshot format version. Increment when fields change.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Maximum snapshot size for deserialization (64MB).
/// Prevents OOM from crafted inputs with inflated length prefixes.
const MAX_SNAPSHOT_SIZE: u64 = 64 * 1024 * 1024;

/// Versioned envelope for terminal snapshots.
#[derive(Serialize, Deserialize)]
pub struct SnapshotEnvelope {
    pub version: u32,
    pub payload: Vec<u8>,
}

/// Serializable terminal state snapshot.
///
/// Contains all persistent state needed to restore a terminal pane.
/// Excludes runtime-only fields: callbacks, pixel metrics, dirty tracking,
/// scroll events, mode action queue, and parser buffers.
#[derive(Serialize, Deserialize)]
pub struct TerminalSnapshot {
    // Grid dimensions
    pub cols: u16,
    pub rows: u16,
    // Ring buffer state
    pub ring_cells: Vec<Cell>,
    pub ring_wrapped: Vec<bool>,
    pub ring_head: usize,
    pub ring_size: usize,
    pub ring_capacity: usize,
    // Cursor state
    pub cursor: CursorState,
    pub saved_cursor: Option<CursorState>,
    // Terminal modes and settings
    pub modes: u32,
    pub tab_stops: Vec<bool>,
    pub scroll_region_top: u16,
    pub scroll_region_bottom: u16,
    // Overflow table (chars > 16 bytes)
    pub overflow: HashMap<(u32, u32), String>,
    pub overflow_ridx: HashMap<u32, Vec<u32>>,
    // Print handler state
    pub wrap_pending: bool,
    pub g0_charset: u8,
    pub g1_charset: u8,
    pub active_charset: u8,
    // Hyperlink table
    pub hyperlink_table: Vec<Option<(String, String)>>,
    pub hyperlink_next_id: u16,
    pub active_hyperlink_id: u16,
    // Settings
    pub cursor_show_interrupt: bool,
}

impl TerminalCore {
    /// Extract persistent state into a serializable snapshot.
    pub fn to_snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            cols: self.cols,
            rows: self.rows,
            ring_cells: self.ring_cells.clone(),
            ring_wrapped: self.ring_wrapped.clone(),
            ring_head: self.ring_head,
            ring_size: self.ring_size,
            ring_capacity: self.ring_capacity,
            cursor: self.cursor.clone(),
            saved_cursor: self.saved_cursor.clone(),
            modes: self.modes,
            tab_stops: self.tab_stops.clone(),
            scroll_region_top: self.scroll_region_top,
            scroll_region_bottom: self.scroll_region_bottom,
            overflow: self.overflow.clone(),
            overflow_ridx: self.overflow_ridx.clone(),
            wrap_pending: self.wrap_pending,
            g0_charset: self.g0_charset,
            g1_charset: self.g1_charset,
            active_charset: self.active_charset,
            hyperlink_table: self.hyperlink_table.clone(),
            hyperlink_next_id: self.hyperlink_next_id,
            active_hyperlink_id: self.active_hyperlink_id,
            cursor_show_interrupt: self.cursor_show_interrupt,
        }
    }

    /// Restore a TerminalCore from a validated snapshot.
    ///
    /// Returns None if the snapshot fails structural invariant checks.
    /// Callbacks are NOT set — caller must re-register them before
    /// processing further data.
    pub fn from_snapshot(snapshot: TerminalSnapshot) -> Option<Self> {
        // Validate structural invariants
        let s = &snapshot;
        if s.cols == 0 || s.rows == 0 {
            return None;
        }
        let expected_cells = s.ring_capacity.checked_mul(s.cols as usize)?;
        if s.ring_cells.len() != expected_cells {
            return None;
        }
        if s.ring_wrapped.len() != s.ring_capacity {
            return None;
        }
        if s.ring_capacity > 0 && s.ring_head >= s.ring_capacity {
            return None;
        }
        if s.ring_size > s.ring_capacity || s.ring_size < s.rows as usize {
            return None;
        }
        if s.cursor.row >= s.rows || s.cursor.col > s.cols {
            return None;
        }
        if s.scroll_region_bottom > s.rows || s.scroll_region_top >= s.scroll_region_bottom {
            return None;
        }

        let dirty_words = (snapshot.rows as usize + 63) / 64;
        let mut dirty = vec![0u64; dirty_words];
        // Mark all rows dirty so first render draws everything
        for word in &mut dirty {
            *word = u64::MAX;
        }

        Some(Self {
            cols: snapshot.cols,
            rows: snapshot.rows,
            ring_cells: snapshot.ring_cells,
            ring_wrapped: snapshot.ring_wrapped,
            ring_head: snapshot.ring_head,
            ring_size: snapshot.ring_size,
            ring_capacity: snapshot.ring_capacity,
            dirty,
            cursor: snapshot.cursor,
            saved_cursor: snapshot.saved_cursor,
            modes: snapshot.modes,
            tab_stops: snapshot.tab_stops,
            overflow: snapshot.overflow,
            overflow_ridx: snapshot.overflow_ridx,
            grapheme_buffer: Vec::new(),
            wrap_pending: snapshot.wrap_pending,
            g0_charset: snapshot.g0_charset,
            g1_charset: snapshot.g1_charset,
            active_charset: snapshot.active_charset,
            kitty_placeholder_active: false,
            scroll_region_top: snapshot.scroll_region_top,
            scroll_region_bottom: snapshot.scroll_region_bottom,
            response_buffer: [0u8; 64],
            response_len: 0,
            cell_width_px: 0,
            cell_height_px: 0,
            scroll_event: None,
            parser: crate::parser::Parser::new(),
            mode_actions: Vec::new(),
            osc_callback: None,
            apc_callback: None,
            dcs_callback: None,
            bell_callback: None,
            device_response_callback: None,
            hyperlink_table: snapshot.hyperlink_table,
            hyperlink_next_id: snapshot.hyperlink_next_id,
            active_hyperlink_id: snapshot.active_hyperlink_id,
            cursor_just_shown: false,
            cursor_show_interrupt: snapshot.cursor_show_interrupt,
        })
    }

    /// Serialize terminal state to bytes with version envelope.
    ///
    /// Returns a versioned binary blob suitable for storage or IPC transfer.
    pub fn snapshot_to_bytes(&self) -> Vec<u8> {
        let snapshot = self.to_snapshot();
        let payload =
            bincode::serialize(&snapshot).expect("snapshot serialization should not fail");
        let envelope = SnapshotEnvelope {
            version: SNAPSHOT_VERSION,
            payload,
        };
        bincode::serialize(&envelope).expect("envelope serialization should not fail")
    }

    /// Restore terminal state from versioned bytes.
    ///
    /// Returns None if the version is incompatible, data is corrupted,
    /// data exceeds size limit, or structural invariants fail.
    /// Callbacks are NOT set on the restored instance.
    pub fn restore_from_bytes(bytes: &[u8]) -> Option<Self> {
        let opts = bincode::DefaultOptions::new()
            .with_limit(MAX_SNAPSHOT_SIZE)
            .with_fixint_encoding()
            .with_little_endian();
        let envelope: SnapshotEnvelope = opts.deserialize(bytes).ok()?;
        if envelope.version != SNAPSHOT_VERSION {
            return None;
        }
        let snapshot: TerminalSnapshot = opts.deserialize(&envelope.payload).ok()?;
        Self::from_snapshot(snapshot)
    }
}

// wasm_bindgen exports for JS access
#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::terminal_core::TerminalCore;

    #[wasm_bindgen]
    impl TerminalCore {
        /// Serialize the terminal state to a binary snapshot.
        pub fn wasm_snapshot_to_bytes(&self) -> Vec<u8> {
            self.snapshot_to_bytes()
        }

        /// Restore a TerminalCore from a binary snapshot.
        /// Returns null if version mismatch or data corruption.
        pub fn wasm_restore_from_bytes(bytes: &[u8]) -> Option<TerminalCore> {
            Self::restore_from_bytes(bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::PackedColor;

    #[test]
    fn test_snapshot_round_trip_empty() {
        let core = TerminalCore::new(80, 24, 100);
        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).expect("restore should succeed");

        assert_eq!(restored.cols, 80);
        assert_eq!(restored.rows, 24);
        assert_eq!(restored.ring_capacity, 124); // 100 + 24
        assert_eq!(restored.cursor.col, 0);
        assert_eq!(restored.cursor.row, 0);
        assert!(restored.cursor.visible);
    }

    #[test]
    fn test_snapshot_preserves_cursor_state() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Move cursor and change attributes
        core.cursor.col = 42;
        core.cursor.row = 10;
        core.cursor.fg = PackedColor::rgb(255, 0, 0);
        core.cursor.style = 2; // bar
        core.cursor.visible = false;

        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).unwrap();

        assert_eq!(restored.cursor.col, 42);
        assert_eq!(restored.cursor.row, 10);
        assert_eq!(restored.cursor.fg, PackedColor::rgb(255, 0, 0));
        assert_eq!(restored.cursor.style, 2);
        assert!(!restored.cursor.visible);
    }

    #[test]
    fn test_snapshot_preserves_cell_data() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Write a character
        let idx = core.ring_head * core.cols as usize; // first cell
        core.ring_cells[idx].set_char("A");
        core.ring_cells[idx].fg = PackedColor::indexed(1);
        core.ring_cells[idx].flags = 0x0001; // bold

        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).unwrap();

        let ridx = restored.ring_head * restored.cols as usize;
        assert_eq!(restored.ring_cells[ridx].char_len, 1);
        assert_eq!(restored.ring_cells[ridx].char_data[0], b'A');
        assert_eq!(restored.ring_cells[ridx].fg, PackedColor::indexed(1));
        assert_eq!(restored.ring_cells[ridx].flags, 0x0001);
    }

    #[test]
    fn test_snapshot_preserves_modes() {
        let mut core = TerminalCore::new(80, 24, 0);
        core.modes = 0x1234;
        core.scroll_region_top = 5;
        core.scroll_region_bottom = 20;

        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).unwrap();

        assert_eq!(restored.modes, 0x1234);
        assert_eq!(restored.scroll_region_top, 5);
        assert_eq!(restored.scroll_region_bottom, 20);
    }

    #[test]
    fn test_snapshot_callbacks_not_set() {
        let core = TerminalCore::new(80, 24, 0);
        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).unwrap();

        assert!(restored.osc_callback.is_none());
        assert!(restored.apc_callback.is_none());
        assert!(restored.dcs_callback.is_none());
        assert!(restored.bell_callback.is_none());
        assert!(restored.device_response_callback.is_none());
    }

    #[test]
    fn test_snapshot_version_mismatch() {
        let core = TerminalCore::new(80, 24, 0);
        let mut bytes = core.snapshot_to_bytes();
        // Corrupt version field (first 4 bytes after bincode length encoding)
        // bincode encodes u32 as 4 little-endian bytes at the start of SnapshotEnvelope
        if bytes.len() >= 12 {
            bytes[8] = 99; // corrupt version
        }
        let result = TerminalCore::restore_from_bytes(&bytes);
        assert!(result.is_none(), "Version mismatch should return None");
    }

    #[test]
    fn test_snapshot_corrupted_data() {
        let result = TerminalCore::restore_from_bytes(&[0, 1, 2, 3]);
        assert!(result.is_none(), "Corrupted data should return None");
    }

    #[test]
    fn test_snapshot_empty_data() {
        let result = TerminalCore::restore_from_bytes(&[]);
        assert!(result.is_none(), "Empty data should return None");
    }

    #[test]
    fn test_snapshot_all_dirty_after_restore() {
        let core = TerminalCore::new(80, 24, 0);
        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).unwrap();

        // All viewport rows should be dirty after restore
        for row in 0..restored.rows {
            assert!(
                restored.is_row_dirty(row),
                "Row {} should be dirty after restore",
                row
            );
        }
    }

    #[test]
    fn test_snapshot_size_reasonable() {
        // 80x24 with 10000 scrollback
        let core = TerminalCore::new(80, 24, 10000);
        let bytes = core.snapshot_to_bytes();
        // Each cell is 32 bytes, 80 cols * 10024 rows = ~25MB raw,
        // but bincode + empty cells should compress well
        assert!(
            bytes.len() < 30 * 1024 * 1024,
            "Snapshot too large: {} bytes",
            bytes.len()
        );
    }

    #[test]
    fn test_snapshot_preserves_hyperlinks() {
        let mut core = TerminalCore::new(80, 24, 0);
        // Index 0 is None sentinel (no hyperlink), so push at index 1
        core.hyperlink_table.push(Some((
            "id=1".to_string(),
            "https://example.com".to_string(),
        )));
        core.hyperlink_next_id = 2;
        core.active_hyperlink_id = 1;

        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).unwrap();

        assert_eq!(restored.hyperlink_table.len(), 2); // [None, Some(...)]
        assert!(restored.hyperlink_table[0].is_none());
        let link = restored.hyperlink_table[1].as_ref().unwrap();
        assert_eq!(link.0, "id=1");
        assert_eq!(link.1, "https://example.com");
        assert_eq!(restored.hyperlink_next_id, 2);
        assert_eq!(restored.active_hyperlink_id, 1);
    }

    #[test]
    fn test_from_snapshot_rejects_invalid_ring_cells_len() {
        let mut core = TerminalCore::new(80, 24, 0);
        let mut snapshot = core.to_snapshot();
        snapshot.ring_cells.push(Cell::EMPTY); // wrong length
        assert!(
            TerminalCore::from_snapshot(snapshot).is_none(),
            "Should reject mismatched ring_cells length"
        );
    }

    #[test]
    fn test_from_snapshot_rejects_invalid_ring_head() {
        let core = TerminalCore::new(80, 24, 0);
        let mut snapshot = core.to_snapshot();
        snapshot.ring_head = snapshot.ring_capacity; // out of bounds
        assert!(
            TerminalCore::from_snapshot(snapshot).is_none(),
            "Should reject ring_head >= ring_capacity"
        );
    }

    #[test]
    fn test_from_snapshot_rejects_invalid_cursor() {
        let core = TerminalCore::new(80, 24, 0);
        let mut snapshot = core.to_snapshot();
        snapshot.cursor.row = 24; // row >= rows
        assert!(
            TerminalCore::from_snapshot(snapshot).is_none(),
            "Should reject cursor.row >= rows"
        );
    }

    #[test]
    fn test_from_snapshot_rejects_zero_dimensions() {
        let core = TerminalCore::new(80, 24, 0);
        let mut snapshot = core.to_snapshot();
        snapshot.cols = 0;
        assert!(
            TerminalCore::from_snapshot(snapshot).is_none(),
            "Should reject zero cols"
        );
    }

    #[test]
    fn test_from_snapshot_rejects_bad_scroll_region() {
        let core = TerminalCore::new(80, 24, 0);
        let mut snapshot = core.to_snapshot();
        snapshot.scroll_region_top = 20;
        snapshot.scroll_region_bottom = 10; // top >= bottom
        assert!(
            TerminalCore::from_snapshot(snapshot).is_none(),
            "Should reject scroll_region_top >= scroll_region_bottom"
        );
    }
}
