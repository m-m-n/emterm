/// Cell accessor impl methods for TerminalCore.
///
/// Provides cell-level read/write operations: set_cell, set_cell_ascii,
/// get_cell_char/width/fg/bg/flags, hyperlink accessors, row packing, and BCE.
use crate::cell::*;
use crate::terminal_core::TerminalCore;

impl TerminalCore {
    // ── Cell access (internal) ───────────────────────────

    pub(crate) fn cell_index(&self, col: u16, row: u16) -> Option<usize> {
        self.viewport_cell_offset(col, row)
    }

    // ── Cell write ───────────────────────────────────────

    pub fn set_cell(
        &mut self,
        col: u16,
        row: u16,
        char_str: &str,
        width: u8,
        fg_tag: u8,
        fg_r: u8,
        fg_g: u8,
        fg_b: u8,
        bg_tag: u8,
        bg_r: u8,
        bg_g: u8,
        bg_b: u8,
        flags: u16,
    ) {
        if let Some(idx) = self.cell_index(col, row) {
            let abs = self.viewport_abs(row) as u32;
            let cell = &mut self.ring_cells[idx];
            cell.set_char(char_str);
            let col32 = col as u32;
            if cell.is_overflow() {
                self.overflow.insert((col32, abs), char_str.to_string());
                overflow_ridx_insert(&mut self.overflow_ridx, abs, col32);
            } else {
                if self.overflow.remove(&(col32, abs)).is_some() {
                    overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
                }
            }
            cell.width = width;
            cell.fg = PackedColor {
                tag: fg_tag,
                r: fg_r,
                g: fg_g,
                b: fg_b,
            };
            cell.bg = PackedColor {
                tag: bg_tag,
                r: bg_r,
                g: bg_g,
                b: bg_b,
            };
            cell.flags = flags;
            self.mark_row_dirty(row);
        }
    }

    pub fn set_cell_ascii(
        &mut self,
        col: u16,
        row: u16,
        byte: u8,
        fg_tag: u8,
        fg_r: u8,
        fg_g: u8,
        fg_b: u8,
        bg_tag: u8,
        bg_r: u8,
        bg_g: u8,
        bg_b: u8,
        flags: u16,
    ) {
        if let Some(idx) = self.cell_index(col, row) {
            let cell = &mut self.ring_cells[idx];
            cell.char_data[0] = byte;
            for b in &mut cell.char_data[1..] {
                *b = 0;
            }
            cell.char_len = 1;
            cell.width = 1;
            cell.fg = PackedColor {
                tag: fg_tag,
                r: fg_r,
                g: fg_g,
                b: fg_b,
            };
            cell.bg = PackedColor {
                tag: bg_tag,
                r: bg_r,
                g: bg_g,
                b: bg_b,
            };
            cell.flags = flags;
            let abs = self.viewport_abs(row) as u32;
            let col32 = col as u32;
            if self.overflow.remove(&(col32, abs)).is_some() {
                overflow_ridx_remove(&mut self.overflow_ridx, abs, col32);
            }
            self.mark_row_dirty(row);
        }
    }

    // ── Cell read ────────────────────────────────────────

    pub fn get_cell_char(&self, col: u16, row: u16) -> String {
        if let Some(idx) = self.cell_index(col, row) {
            let abs = self.viewport_abs(row) as u32;
            let cell = &self.ring_cells[idx];
            if cell.is_overflow() {
                self.overflow
                    .get(&(col as u32, abs))
                    .cloned()
                    .unwrap_or_default()
            } else {
                cell.get_char_inline().unwrap_or(" ").to_string()
            }
        } else {
            " ".to_string()
        }
    }

    pub fn get_cell_width(&self, col: u16, row: u16) -> u8 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].width)
            .unwrap_or(1)
    }

    pub fn get_cell_fg(&self, col: u16, row: u16) -> u32 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].fg.to_u32())
            .unwrap_or(0)
    }

    pub fn get_cell_bg(&self, col: u16, row: u16) -> u32 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].bg.to_u32())
            .unwrap_or(0)
    }

    pub fn get_cell_flags(&self, col: u16, row: u16) -> u16 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].flags)
            .unwrap_or(0)
    }

    pub fn get_cell_hyperlink_id(&self, col: u16, row: u16) -> u16 {
        self.cell_index(col, row)
            .map(|i| self.ring_cells[i].hyperlink_id)
            .unwrap_or(0)
    }

    /// Get hyperlink URI by ID. Returns empty string if not found.
    pub fn get_hyperlink_uri(&self, id: u16) -> String {
        if id == 0 {
            return String::new();
        }
        self.hyperlink_table
            .get(id as usize)
            .and_then(|entry| entry.as_ref())
            .map(|(_, uri)| uri.clone())
            .unwrap_or_default()
    }

    /// Get hyperlink params by ID. Returns empty string if not found.
    pub fn get_hyperlink_params(&self, id: u16) -> String {
        if id == 0 {
            return String::new();
        }
        self.hyperlink_table
            .get(id as usize)
            .and_then(|entry| entry.as_ref())
            .map(|(params, _)| params.clone())
            .unwrap_or_default()
    }

    // ── Diagnostics ──────────────────────────────────────

    /// Compute a stable FNV-1a 32-bit hash of all visible viewport cells
    /// (codepoint bytes + width + fg + bg + flags). Frontend diagnostics
    /// compare this against the canvas pixel hash to tell whether a freeze
    /// is in the renderer (grid changed but pixels didn't) or upstream
    /// (grid never changed). No allocation; cost is O(cols * rows) bytes
    /// hashed and runs only on mux switches.
    pub fn grid_content_hash(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for row in 0..self.rows {
            for col in 0..self.cols {
                let Some(idx) = self.viewport_cell_offset(col, row) else {
                    continue;
                };
                let cell = &self.ring_cells[idx];
                let len = if cell.char_len == 0xFF {
                    16
                } else {
                    (cell.char_len as usize).min(16)
                };
                for b in &cell.char_data[..len] {
                    h ^= *b as u32;
                    h = h.wrapping_mul(16777619);
                }
                h ^= cell.width as u32;
                h = h.wrapping_mul(16777619);
                let fg = cell.fg.to_u32();
                let bg = cell.bg.to_u32();
                let flags = cell.flags as u32;
                for w in [fg, bg, flags] {
                    for shift in [0, 8, 16, 24] {
                        h ^= (w >> shift) & 0xff;
                        h = h.wrapping_mul(16777619);
                    }
                }
            }
        }
        h
    }

    // ── Batch cell read ──────────────────────────────────

    pub fn get_row_packed(&self, row: u16) -> Vec<u8> {
        if row >= self.rows {
            return Vec::new();
        }
        let abs = self.viewport_abs(row);
        self.pack_row_abs(abs)
    }

    // ── BCE (Background Color Erase) ────────────────────

    /// Create a blank cell with the cursor's current background color (BCE).
    pub(crate) fn bce_cell(&self) -> Cell {
        let mut cell = Cell::EMPTY;
        cell.bg = self.cursor.bg;
        cell
    }

    // ── Wide-pair partner blanking primitive ─────────────

    /// Blank a wide-pair half (an orphaned spacer at width 0, or a base at
    /// width 2) at `(col, row)` in place: rewrite it to a width-1 space,
    /// preserving fg/bg/flags/hyperlink, remove any overflow-table entry
    /// (and its reverse-index entry) for the position, and mark the row
    /// dirty.
    ///
    /// Self-guarding (no precondition on the caller): a no-op, with no
    /// mutation and no panic, when `(col, row)` does not resolve to a cell
    /// or when the cell's current width is neither 0 nor 2 (e.g. already
    /// width 1).
    ///
    /// This is the single repair for the D2 invariant (no width-0 cell may
    /// remain whose left neighbour is not a width-2 base, and no width-2
    /// base may remain whose right neighbour is not a width-0 spacer,
    /// after any write completes). It is reached from exactly these call
    /// sites, and no others:
    ///
    /// - the print path's grapheme writer, before an overwrite (rules
    ///   R1/R2) and before writing a wide-pair placeholder (rule R3);
    /// - the print path's ASCII writer, before an overwrite (rules R1/R2);
    /// - the print path's widened-base relocation-by-wrap step (rules
    ///   R1/R2/R3);
    /// - the PTY-dispatch ASCII fast path's write step, before an
    ///   overwrite (rules R1/R2) — a distinct code path from the print
    ///   path's ASCII writer above, for the same byte;
    /// - the ICH/DCH edit path's edge repair;
    /// - the range-erase edge-repair chokepoint.
    pub(crate) fn blank_wide_pair_half(&mut self, col: u16, row: u16) {
        let Some(idx) = self.cell_index(col, row) else {
            return;
        };
        let width = self.ring_cells[idx].width;
        if width != 0 && width != 2 {
            return;
        }
        if !self.overflow.is_empty() {
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
}

#[cfg(test)]
mod blank_wide_pair_half_tests {
    use super::*;
    use crate::cell::{PackedColor, STYLE_BOLD};

    fn make_core() -> TerminalCore {
        TerminalCore::new(10, 3, 0)
    }

    // AC-1 (TS-2): a width-2 base cell is rewritten to a width-1 space.
    #[test]
    fn width2_base_becomes_width1_space() {
        let mut core = make_core();
        let idx = core.cell_index(2, 0).unwrap();
        core.ring_cells[idx].set_char("世");
        core.ring_cells[idx].width = 2;
        core.clear_dirty();

        core.blank_wide_pair_half(2, 0);

        assert_eq!(core.get_cell_char(2, 0), " ");
        assert_eq!(core.get_cell_width(2, 0), 1);
    }

    // AC-1 (TS-2): a width-0 spacer cell is rewritten to a width-1 space.
    #[test]
    fn width0_spacer_becomes_width1_space() {
        let mut core = make_core();
        let idx = core.cell_index(3, 0).unwrap();
        core.ring_cells[idx].char_data = [0; 16];
        core.ring_cells[idx].char_len = 0;
        core.ring_cells[idx].width = 0;
        core.clear_dirty();

        core.blank_wide_pair_half(3, 0);

        assert_eq!(core.get_cell_char(3, 0), " ");
        assert_eq!(core.get_cell_width(3, 0), 1);
    }

    // AC-1 (TS-2): fg/bg/flags/hyperlink survive the blank.
    #[test]
    fn preserves_fg_bg_flags_hyperlink() {
        let mut core = make_core();
        let idx = core.cell_index(1, 0).unwrap();
        core.ring_cells[idx].set_char("世");
        core.ring_cells[idx].width = 2;
        core.ring_cells[idx].fg = PackedColor::indexed(9);
        core.ring_cells[idx].bg = PackedColor::indexed(3);
        core.ring_cells[idx].flags = STYLE_BOLD;
        core.ring_cells[idx].hyperlink_id = 7;

        core.blank_wide_pair_half(1, 0);

        assert_eq!(core.get_cell_char(1, 0), " ");
        assert_eq!(core.get_cell_width(1, 0), 1);
        assert_eq!(
            PackedColor::from_u32(core.get_cell_fg(1, 0)),
            PackedColor::indexed(9)
        );
        assert_eq!(
            PackedColor::from_u32(core.get_cell_bg(1, 0)),
            PackedColor::indexed(3)
        );
        assert_eq!(core.get_cell_flags(1, 0), STYLE_BOLD);
        assert_eq!(core.get_cell_hyperlink_id(1, 0), 7);
    }

    // AC-1 (TS-2): a removed overflow entry also removes its reverse-index
    // entry.
    #[test]
    fn removes_overflow_entry_and_reverse_index() {
        let mut core = make_core();
        let idx = core.cell_index(0, 0).unwrap();
        core.ring_cells[idx].char_len = 0xFF; // mark as overflow
        core.ring_cells[idx].width = 2;
        core.overflow
            .insert((0, 0), "long-overflow-content".to_string());
        overflow_ridx_insert(&mut core.overflow_ridx, 0, 0);
        assert!(core.overflow.contains_key(&(0, 0)));
        assert!(
            core.overflow_ridx
                .get(&0)
                .is_some_and(|cols| cols.contains(&0))
        );

        core.blank_wide_pair_half(0, 0);

        assert!(!core.overflow.contains_key(&(0, 0)));
        assert!(
            !core
                .overflow_ridx
                .get(&0)
                .is_some_and(|cols| cols.contains(&0))
        );
        assert_eq!(core.get_cell_char(0, 0), " ");
        assert_eq!(core.get_cell_width(0, 0), 1);
    }

    // AC-1 (TS-2): the row is marked dirty.
    #[test]
    fn marks_row_dirty() {
        let mut core = make_core();
        let idx = core.cell_index(2, 1).unwrap();
        core.ring_cells[idx].width = 0;
        core.clear_dirty();
        assert!(!core.is_row_dirty(1));

        core.blank_wide_pair_half(2, 1);

        assert!(core.is_row_dirty(1));
    }

    // AC-2 (TS-3): a width-1 cell is a strict no-op (no mutation, no dirty
    // mark).
    #[test]
    fn width1_cell_is_strict_noop() {
        let mut core = make_core();
        core.set_cell_ascii(4, 0, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0);
        core.clear_dirty();

        core.blank_wide_pair_half(4, 0);

        assert_eq!(core.get_cell_char(4, 0), "A");
        assert_eq!(core.get_cell_width(4, 0), 1);
        assert!(!core.is_row_dirty(0));
    }

    // AC-2 (TS-3): an out-of-bounds position is a no-op with no panic.
    #[test]
    fn out_of_bounds_position_is_noop_no_panic() {
        let mut core = make_core();
        core.blank_wide_pair_half(100, 0); // col OOB
        core.blank_wide_pair_half(0, 100); // row OOB
        // Reaching this point without panicking is the assertion.
    }
}
