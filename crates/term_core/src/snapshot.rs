/// Dedicated snapshot type for terminal state serialization.
///
/// Separated from the runtime `TerminalCore` to cleanly exclude
/// non-serializable fields (callbacks, pixel metrics) and to provide
/// a versioned envelope for forward compatibility.
use std::collections::HashMap;

use bincode::Options;
use serde::{Deserialize, Serialize};

use std::collections::VecDeque;

use crate::cell::Cell;
use crate::char_table::CharTable;
use crate::style_table::StyleTable;
use crate::terminal_core::{CursorState, TerminalCore};

/// Current snapshot format version. Increment when fields change.
pub const SNAPSHOT_VERSION: u32 = 2;
/// Legacy V1: kept readable so old persisted snapshots still load
/// (with scrollback dropped — see SPEC §Migration).
pub const SNAPSHOT_VERSION_V1: u32 = 1;

/// Maximum snapshot size for deserialization (64MB).
/// Prevents OOM from crafted inputs with inflated length prefixes.
const MAX_SNAPSHOT_SIZE: u64 = 64 * 1024 * 1024;

/// Versioned envelope for terminal snapshots.
#[derive(Serialize, Deserialize)]
pub struct SnapshotEnvelope {
    pub version: u32,
    pub payload: Vec<u8>,
}

/// Serializable terminal state snapshot (V2 layout, current).
///
/// Viewport rows are stored as `Cell`s; scrollback rows as `SlimCell`s plus
/// the intern tables required to interpret them.
#[derive(Serialize, Deserialize)]
pub(crate) struct TerminalSnapshot {
    // Grid dimensions
    pub cols: u16,
    pub rows: u16,
    // Viewport ring buffer state
    pub ring_cells: Vec<Cell>,
    pub ring_wrapped: Vec<bool>,
    pub ring_head: usize,
    pub ring_size: usize,
    pub ring_capacity: usize,
    // Compressed scrollback (V2)
    pub scrollback_slim: Vec<Vec<crate::slim_cell::SlimCell>>,
    pub scrollback_wrapped: Vec<bool>,
    pub scrollback_capacity: usize,
    pub style_table: SerializedStyleTable,
    pub char_table: SerializedCharTable,
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

/// Legacy V1 snapshot: read-only; loaded with empty scrollback per spec.
#[derive(Serialize, Deserialize)]
pub(crate) struct TerminalSnapshotV1 {
    pub cols: u16,
    pub rows: u16,
    pub ring_cells: Vec<Cell>,
    pub ring_wrapped: Vec<bool>,
    pub ring_head: usize,
    pub ring_size: usize,
    pub ring_capacity: usize,
    pub cursor: CursorState,
    pub saved_cursor: Option<CursorState>,
    pub modes: u32,
    pub tab_stops: Vec<bool>,
    pub scroll_region_top: u16,
    pub scroll_region_bottom: u16,
    pub overflow: HashMap<(u32, u32), String>,
    pub overflow_ridx: HashMap<u32, Vec<u32>>,
    pub wrap_pending: bool,
    pub g0_charset: u8,
    pub g1_charset: u8,
    pub active_charset: u8,
    pub hyperlink_table: Vec<Option<(String, String)>>,
    pub hyperlink_next_id: u16,
    pub active_hyperlink_id: u16,
    pub cursor_show_interrupt: bool,
}

/// Serialized snapshot of a `StyleTable`. Mirrors the in-memory layout for
/// faithful round-trip including refcounts and free list.
#[derive(Serialize, Deserialize)]
pub(crate) struct SerializedStyleTable {
    pub storage: Vec<crate::style_table::StyleEntry>,
    pub refcount: Vec<u32>,
    pub free_list: Vec<u16>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SerializedCharTable {
    pub storage: Vec<String>,
    pub refcount: Vec<u32>,
    pub free_list: Vec<u32>,
}

impl TerminalCore {
    /// Extract persistent state into a serializable snapshot (V2 layout).
    pub(crate) fn to_snapshot(&self) -> TerminalSnapshot {
        let (style_storage, style_refcount, style_free) = self.styles.snapshot();
        let (char_storage, char_refcount, char_free) = self.chars.snapshot();
        TerminalSnapshot {
            cols: self.cols,
            rows: self.rows,
            ring_cells: self.ring_cells.clone(),
            ring_wrapped: self.ring_wrapped.clone(),
            ring_head: self.ring_head,
            ring_size: self.ring_size,
            ring_capacity: self.ring_capacity,
            scrollback_slim: self.scrollback_slim.iter().cloned().collect(),
            scrollback_wrapped: self.scrollback_wrapped.iter().copied().collect(),
            scrollback_capacity: self.scrollback_capacity,
            style_table: SerializedStyleTable {
                storage: style_storage,
                refcount: style_refcount,
                free_list: style_free,
            },
            char_table: SerializedCharTable {
                storage: char_storage,
                refcount: char_refcount,
                free_list: char_free,
            },
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

    /// Restore a TerminalCore from a validated V2 snapshot.
    ///
    /// Returns None if the snapshot fails structural invariant checks.
    /// Callbacks are NOT set — caller must re-register them before
    /// processing further data.
    pub(crate) fn from_snapshot(snapshot: TerminalSnapshot) -> Option<Self> {
        // Validate structural invariants
        let s = &snapshot;
        if s.cols == 0 || s.rows == 0 {
            return None;
        }
        let expected_cells = (s.rows as usize).checked_mul(s.cols as usize)?;
        if s.ring_cells.len() != expected_cells {
            return None;
        }
        if s.ring_wrapped.len() != s.rows as usize {
            return None;
        }
        if s.ring_head >= s.rows as usize {
            return None;
        }
        if s.ring_size != s.rows as usize {
            return None;
        }
        if s.cursor.row >= s.rows || s.cursor.col > s.cols {
            return None;
        }
        if s.scroll_region_bottom >= s.rows || s.scroll_region_top >= s.scroll_region_bottom {
            return None;
        }
        if s.scrollback_slim.len() != s.scrollback_wrapped.len() {
            return None;
        }
        if s.scrollback_slim.len() > s.scrollback_capacity {
            return None;
        }
        // Each scrollback row must match cols.
        for row in &s.scrollback_slim {
            if row.len() != s.cols as usize {
                return None;
            }
        }

        // Build free-set membership tests up-front (before moving free_list
        // into the table constructors below).
        let style_free_set: std::collections::HashSet<u16> =
            snapshot.style_table.free_list.iter().copied().collect();
        let char_free_set: std::collections::HashSet<u32> =
            snapshot.char_table.free_list.iter().copied().collect();

        // Restore intern tables.
        let styles = StyleTable::from_snapshot(
            snapshot.style_table.storage,
            snapshot.style_table.refcount,
            snapshot.style_table.free_list,
        )?;
        let chars = CharTable::from_snapshot(
            snapshot.char_table.storage,
            snapshot.char_table.refcount,
            snapshot.char_table.free_list,
        )?;
        // Validate scrollback IDs against table sizes AND ensure they aren't
        // referencing freed slots (defense against crafted snapshots that would
        // cause silent style/char loss or refcount underflow on later eviction).
        for row in &snapshot.scrollback_slim {
            for slim in row {
                if (slim.style_id as usize) >= styles.slot_count() {
                    return None;
                }
                if style_free_set.contains(&slim.style_id) {
                    return None;
                }
                if slim.is_char_table() {
                    if (slim.char_ref as usize) >= chars.slot_count() {
                        return None;
                    }
                    if char_free_set.contains(&slim.char_ref) {
                        return None;
                    }
                }
            }
        }

        let dirty_words = (snapshot.rows as usize + 63) / 64;
        let mut dirty = vec![0u64; dirty_words];
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
            scrollback_slim: VecDeque::from(snapshot.scrollback_slim),
            scrollback_wrapped: VecDeque::from(snapshot.scrollback_wrapped),
            scrollback_capacity: snapshot.scrollback_capacity,
            // Snapshot restore rebuilds scrollback from scratch, so the
            // eviction counter starts at a fresh baseline. Consumers
            // re-baseline off the same restore event.
            scrollback_evicted_total: 0,
            scrollback_bypass: false,
            virtual_scrollback_len: 0,
            bypass_b_mark_texts: HashMap::new(),
            styles,
            chars,
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
            callbacks: None,
            hyperlink_table: snapshot.hyperlink_table,
            hyperlink_next_id: snapshot.hyperlink_next_id,
            active_hyperlink_id: snapshot.active_hyperlink_id,
            cursor_just_shown: false,
            cursor_show_interrupt: snapshot.cursor_show_interrupt,
            // Snapshot restore starts a fresh parse frame; no in-flight marks.
            pending_prompt_marks: VecDeque::new(),
            pending_fold_marks: VecDeque::new(),
            // Snapshot replay cores process inner content only (no mux
            // transport frames), so no app-layer OSC override is needed.
            osc_app_params: Vec::new(),
        })
    }

    /// Restore a TerminalCore from a legacy V1 snapshot. Scrollback is
    /// dropped because V1 stored it inline in `ring_cells` (a layout we no
    /// longer support).
    pub(crate) fn from_snapshot_v1(snapshot: TerminalSnapshotV1) -> Option<Self> {
        if snapshot.cols == 0 || snapshot.rows == 0 {
            return None;
        }
        let cols = snapshot.cols as usize;
        let rows = snapshot.rows as usize;
        let expected_cells = rows.checked_mul(cols)?;
        // V1 ring_cells = ring_capacity * cols. Take only the viewport rows.
        let v1_ring_capacity = snapshot.ring_capacity;
        let v1_cells_expected = v1_ring_capacity.checked_mul(cols)?;
        if snapshot.ring_cells.len() != v1_cells_expected {
            return None;
        }
        if snapshot.ring_wrapped.len() != v1_ring_capacity {
            return None;
        }
        if v1_ring_capacity > 0 && snapshot.ring_head >= v1_ring_capacity {
            return None;
        }
        if snapshot.ring_size > v1_ring_capacity || snapshot.ring_size < rows {
            return None;
        }
        // Extract the viewport rows (last `rows` of the ring).
        let mut viewport_cells = vec![Cell::EMPTY; expected_cells];
        let mut viewport_wrapped = vec![false; rows];
        for r in 0..rows {
            let v1_abs = (snapshot.ring_head + snapshot.ring_size - rows + r) % v1_ring_capacity;
            let src_base = v1_abs * cols;
            let dst_base = r * cols;
            viewport_cells[dst_base..dst_base + cols]
                .copy_from_slice(&snapshot.ring_cells[src_base..src_base + cols]);
            viewport_wrapped[r] = snapshot.ring_wrapped[v1_abs];
        }
        // Filter overflow table to entries inside the new viewport (best-effort).
        let mut new_overflow: HashMap<(u32, u32), String> = HashMap::new();
        for r in 0..rows {
            let v1_abs = (snapshot.ring_head + snapshot.ring_size - rows + r) % v1_ring_capacity;
            let v1_abs32 = v1_abs as u32;
            for c in 0..cols {
                if let Some(s) = snapshot.overflow.get(&(c as u32, v1_abs32)) {
                    new_overflow.insert((c as u32, r as u32), s.clone());
                }
            }
        }
        let new_overflow_ridx = crate::cell::overflow_ridx_rebuild(&new_overflow);

        let scrollback_capacity = v1_ring_capacity.saturating_sub(rows);
        let dirty_words = (rows + 63) / 64;
        let mut dirty = vec![0u64; dirty_words];
        for word in &mut dirty {
            *word = u64::MAX;
        }

        if snapshot.cursor.row >= snapshot.rows || snapshot.cursor.col > snapshot.cols {
            return None;
        }
        if snapshot.scroll_region_bottom >= snapshot.rows
            || snapshot.scroll_region_top >= snapshot.scroll_region_bottom
        {
            return None;
        }

        Some(Self {
            cols: snapshot.cols,
            rows: snapshot.rows,
            ring_cells: viewport_cells,
            ring_wrapped: viewport_wrapped,
            ring_head: 0,
            ring_size: rows,
            ring_capacity: scrollback_capacity + rows,
            scrollback_slim: VecDeque::new(),
            scrollback_wrapped: VecDeque::new(),
            scrollback_capacity,
            // Restored viewport has no scrollback yet → fresh baseline.
            scrollback_evicted_total: 0,
            scrollback_bypass: false,
            virtual_scrollback_len: 0,
            bypass_b_mark_texts: HashMap::new(),
            styles: StyleTable::new(),
            chars: CharTable::new(),
            dirty,
            cursor: snapshot.cursor,
            saved_cursor: snapshot.saved_cursor,
            modes: snapshot.modes,
            tab_stops: snapshot.tab_stops,
            overflow: new_overflow,
            overflow_ridx: new_overflow_ridx,
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
            callbacks: None,
            hyperlink_table: snapshot.hyperlink_table,
            hyperlink_next_id: snapshot.hyperlink_next_id,
            active_hyperlink_id: snapshot.active_hyperlink_id,
            cursor_just_shown: false,
            cursor_show_interrupt: snapshot.cursor_show_interrupt,
            // Snapshot restore starts a fresh parse frame; no in-flight marks.
            pending_prompt_marks: VecDeque::new(),
            pending_fold_marks: VecDeque::new(),
            // Snapshot replay cores process inner content only (no mux
            // transport frames), so no app-layer OSC override is needed.
            osc_app_params: Vec::new(),
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
    /// Supports current `SNAPSHOT_VERSION` (V2) and the legacy V1 layout
    /// (read-only, scrollback discarded per SPEC §Migration). Returns
    /// `None` for unknown versions, data corruption, oversize input, or
    /// structural invariant failures. Callbacks are NOT set on the
    /// restored instance.
    pub fn restore_from_bytes(bytes: &[u8]) -> Option<Self> {
        let opts = bincode::DefaultOptions::new()
            .with_limit(MAX_SNAPSHOT_SIZE)
            .with_fixint_encoding()
            .with_little_endian();
        let envelope: SnapshotEnvelope = opts.deserialize(bytes).ok()?;
        match envelope.version {
            v if v == SNAPSHOT_VERSION => {
                let snapshot: TerminalSnapshot = opts.deserialize(&envelope.payload).ok()?;
                Self::from_snapshot(snapshot)
            }
            v if v == SNAPSHOT_VERSION_V1 => {
                let snapshot: TerminalSnapshotV1 = opts.deserialize(&envelope.payload).ok()?;
                Self::from_snapshot_v1(snapshot)
            }
            _ => None,
        }
    }
}

// Snapshot serialization is exposed as plain `snapshot_to_bytes` /
// `restore_from_bytes` on TerminalCore (see body above). The wasm/ thin
// wrapper re-exports these via wasm-bindgen at parity with the previous
// `wasm_snapshot_to_bytes` / `wasm_restore_from_bytes` JS names.

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

        assert!(restored.callbacks.is_none());
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
        let core = TerminalCore::new(80, 24, 0);
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
        snapshot.ring_head = snapshot.rows as usize; // out of bounds (>= rows)
        assert!(
            TerminalCore::from_snapshot(snapshot).is_none(),
            "Should reject ring_head >= rows"
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

    // ── Phase 4: V2 + scrollback round-trip ──────────────

    #[test]
    fn test_snapshot_v2_preserves_scrollback() {
        let mut core = TerminalCore::new(10, 3, 5);
        for r in 0..3 {
            core.set_cell(0, r, &format!("{r}"), 1, 2, 100, 50, 25, 0, 0, 0, 0, 0);
        }
        // Push 3 lines into scrollback.
        core.scroll_up_internal(3);
        assert_eq!(core.get_scrollback_length(), 3);

        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).expect("V2 should round-trip");

        assert_eq!(restored.get_scrollback_length(), 3);
        assert_eq!(restored.get_scrollback_text(0), "0");
        assert_eq!(restored.get_scrollback_text(1), "1");
        assert_eq!(restored.get_scrollback_text(2), "2");
    }

    #[test]
    fn test_snapshot_v2_round_trip_with_zwj() {
        let mut core = TerminalCore::new(10, 3, 5);
        let zwj = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        core.set_cell(0, 0, zwj, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.scroll_up_internal(1);

        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).expect("V2 should round-trip");

        let text = restored.get_scrollback_text(0);
        assert!(
            text.contains(zwj),
            "expected ZWJ family in scrollback, got '{text}'"
        );
    }

    #[test]
    fn test_snapshot_v2_rebuilt_tables_match() {
        let mut core = TerminalCore::new(10, 3, 5);
        for i in 0..3u32 {
            core.set_cell(0, 0, "X", 1, 2, i as u8, 0, 0, 0, 0, 0, 0, 0);
            core.scroll_up_internal(1);
        }
        let bytes = core.snapshot_to_bytes();
        let restored = TerminalCore::restore_from_bytes(&bytes).expect("V2 should round-trip");
        let (live_styles, live_chars) = restored.rebuild_intern_tables_from_ring();
        assert_eq!(live_styles, restored.styles.live_entries());
        assert_eq!(live_chars, restored.chars.live_entries());
    }

    #[test]
    fn test_snapshot_v2_rejects_invalid_scrollback_id() {
        let mut core = TerminalCore::new(10, 3, 5);
        for r in 0..3 {
            core.set_cell(0, r, &format!("{r}"), 1, 2, 100, 50, 25, 0, 0, 0, 0, 0);
        }
        core.scroll_up_internal(3);

        let mut snapshot = core.to_snapshot();
        // Corrupt: set a slim cell's style_id beyond the table's slot count.
        if let Some(row) = snapshot.scrollback_slim.get_mut(0) {
            if let Some(slim) = row.get_mut(0) {
                slim.style_id = u16::MAX;
            }
        }
        let result = TerminalCore::from_snapshot(snapshot);
        assert!(result.is_none(), "should reject invalid style_id");
    }

    #[test]
    fn test_snapshot_v2_unknown_version_rejected() {
        let core = TerminalCore::new(80, 24, 0);
        let mut bytes = core.snapshot_to_bytes();
        if bytes.len() >= 12 {
            bytes[8] = 99;
        }
        assert!(TerminalCore::restore_from_bytes(&bytes).is_none());
    }

    #[test]
    fn test_snapshot_v1_dropped_scrollback() {
        // Manually craft a V1 envelope with non-empty scrollback.
        use bincode::Options;
        let opts = bincode::DefaultOptions::new()
            .with_limit(64 * 1024 * 1024)
            .with_fixint_encoding()
            .with_little_endian();
        // Build a simple V1 snapshot structure.
        let cols = 5u16;
        let rows = 2u16;
        let scrollback_lines = 2u32;
        let ring_capacity = scrollback_lines as usize + rows as usize; // 4
        let v1 = TerminalSnapshotV1 {
            cols,
            rows,
            ring_cells: vec![Cell::EMPTY; ring_capacity * cols as usize],
            ring_wrapped: vec![false; ring_capacity],
            ring_head: 0,
            ring_size: ring_capacity, // full = 2 scrollback + 2 viewport
            ring_capacity,
            cursor: CursorState::new(),
            saved_cursor: None,
            modes: 0,
            tab_stops: vec![false; cols as usize],
            scroll_region_top: 0,
            scroll_region_bottom: rows.saturating_sub(1),
            overflow: HashMap::new(),
            overflow_ridx: HashMap::new(),
            wrap_pending: false,
            g0_charset: 0,
            g1_charset: 0,
            active_charset: 0,
            hyperlink_table: vec![None],
            hyperlink_next_id: 1,
            active_hyperlink_id: 0,
            cursor_show_interrupt: false,
        };
        let payload = opts.serialize(&v1).expect("v1 serialize");
        let envelope = SnapshotEnvelope {
            version: SNAPSHOT_VERSION_V1,
            payload,
        };
        let bytes = opts.serialize(&envelope).expect("envelope serialize");
        let restored = TerminalCore::restore_from_bytes(&bytes).expect("V1 should load");
        // V1 loads with empty scrollback per spec.
        assert_eq!(restored.get_scrollback_length(), 0);
        assert_eq!(restored.cols(), cols);
        assert_eq!(restored.rows(), rows);
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
