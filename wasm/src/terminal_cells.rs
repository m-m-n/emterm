/// Cell accessor impl methods for TerminalCore.
///
/// Provides cell-level read/write operations: set_cell, set_cell_ascii,
/// get_cell_char/width/fg/bg/flags, hyperlink accessors, row packing, and BCE.
use wasm_bindgen::prelude::*;

use crate::cell::*;
use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
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
                let Some(idx) = self.viewport_cell_offset(col, row) else { continue };
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
}
