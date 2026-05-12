/// Cell data structures for terminal grid.
///
/// Each Cell is 32 bytes packed, storing character data (UTF-8 inline),
/// display width, foreground/background colors, and style flags.
use std::collections::HashMap;

// ── Style flag constants (u16 bitfield) ──────────────────

pub const STYLE_BOLD: u16 = 0x0001;
pub const STYLE_DIM: u16 = 0x0002;
pub const STYLE_ITALIC: u16 = 0x0004;
pub const STYLE_UNDERLINE: u16 = 0x0008;
pub const STYLE_BLINK: u16 = 0x0010;
pub const STYLE_REVERSE: u16 = 0x0020;
pub const STYLE_HIDDEN: u16 = 0x0040;
pub const STYLE_STRIKETHROUGH: u16 = 0x0080;

// ── PackedColor ──────────────────────────────────────────

/// Color packed into 4 bytes: tag + 3 payload bytes.
///   tag=0: Default, tag=1: Indexed(index), tag=2: RGB(r,g,b)
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct PackedColor {
    pub tag: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl PackedColor {
    pub const DEFAULT: Self = Self {
        tag: 0,
        r: 0,
        g: 0,
        b: 0,
    };

    pub fn indexed(index: u8) -> Self {
        Self {
            tag: 1,
            r: index,
            g: 0,
            b: 0,
        }
    }

    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { tag: 2, r, g, b }
    }

    /// Pack into a u32 for JS boundary: tag<<24 | r<<16 | g<<8 | b
    pub fn to_u32(self) -> u32 {
        (self.tag as u32) << 24 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }

    /// Unpack from u32.
    #[cfg(test)]
    pub fn from_u32(v: u32) -> Self {
        Self {
            tag: (v >> 24) as u8,
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }
}

// ── Cell ─────────────────────────────────────────────────

/// A single terminal cell, 32 bytes packed.
///
/// char_data stores UTF-8 inline (up to 16 bytes). For graphemes exceeding
/// 16 bytes, char_len is set to 0xFF and the data is stored in the
/// TerminalCore overflow side table.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct Cell {
    pub char_data: [u8; 16],
    pub char_len: u8,
    pub width: u8,
    pub fg: PackedColor,
    pub bg: PackedColor,
    pub flags: u16,
    pub underline_style: u8,
    pub underline_color: [u8; 3],
    pub hyperlink_id: u16,
}

impl Cell {
    pub const EMPTY: Self = Self {
        char_data: [b' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        char_len: 1,
        width: 1,
        fg: PackedColor::DEFAULT,
        bg: PackedColor::DEFAULT,
        flags: 0,
        underline_style: 0,
        underline_color: [0; 3],
        hyperlink_id: 0,
    };

    /// Create a cell from a UTF-8 string slice. Returns true if inline, false if overflow.
    /// For overflow, caller must store in side table separately.
    pub fn set_char(&mut self, s: &str) {
        let bytes = s.as_bytes();
        if bytes.len() <= 16 {
            self.char_data[..bytes.len()].copy_from_slice(bytes);
            // Zero remaining bytes
            for b in &mut self.char_data[bytes.len()..] {
                *b = 0;
            }
            self.char_len = bytes.len() as u8;
        } else {
            // Mark as overflow; caller stores in side table
            self.char_data = [0; 16];
            self.char_len = 0xFF;
        }
    }

    /// Get the character as a string slice (inline only).
    /// Returns None if overflow (char_len == 0xFF).
    pub fn get_char_inline(&self) -> Option<&str> {
        if self.char_len == 0xFF {
            return None;
        }
        let len = self.char_len as usize;
        std::str::from_utf8(&self.char_data[..len]).ok()
    }

    pub fn is_overflow(&self) -> bool {
        self.char_len == 0xFF
    }
}

// ── Overflow side table ──────────────────────────────────

pub type OverflowTable = HashMap<(u32, u32), String>;

/// Reverse index: row → list of column keys with overflow entries.
pub type OverflowRowIndex = HashMap<u32, Vec<u32>>;

/// Remove overflow entries for a specific row.
pub fn overflow_clear_row(table: &mut OverflowTable, row: u32) {
    table.retain(|&(_, r), _| r != row);
}

/// Remove overflow entries for a row/col range.
pub fn overflow_clear_range(table: &mut OverflowTable, row: u32, start_col: u32, end_col: u32) {
    table.retain(|&(c, r), _| r != row || c < start_col || c >= end_col);
}

// ── Reverse index helpers ────────────────────────────────

/// Add a (row, col) entry to the reverse index.
pub fn overflow_ridx_insert(ridx: &mut OverflowRowIndex, row: u32, col: u32) {
    let cols = ridx.entry(row).or_default();
    if !cols.contains(&col) {
        cols.push(col);
    }
}

/// Remove a (row, col) entry from the reverse index.
pub fn overflow_ridx_remove(ridx: &mut OverflowRowIndex, row: u32, col: u32) {
    if let Some(cols) = ridx.get_mut(&row) {
        cols.retain(|&c| c != col);
        if cols.is_empty() {
            ridx.remove(&row);
        }
    }
}

/// Remove all entries for a given row from the reverse index.
pub fn overflow_ridx_clear_row(ridx: &mut OverflowRowIndex, row: u32) {
    ridx.remove(&row);
}

/// Remove entries in a column range for a given row from the reverse index.
pub fn overflow_ridx_clear_range(
    ridx: &mut OverflowRowIndex,
    row: u32,
    start_col: u32,
    end_col: u32,
) {
    if let Some(cols) = ridx.get_mut(&row) {
        cols.retain(|&c| c < start_col || c >= end_col);
        if cols.is_empty() {
            ridx.remove(&row);
        }
    }
}

/// Rebuild reverse index from the overflow table.
pub fn overflow_ridx_rebuild(table: &OverflowTable) -> OverflowRowIndex {
    let mut ridx = OverflowRowIndex::new();
    for &(col, row) in table.keys() {
        ridx.entry(row).or_default().push(col);
    }
    ridx
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── PackedColor tests ────────────────────────────────

    #[test]
    fn test_packed_color_default() {
        let c = PackedColor::DEFAULT;
        assert_eq!(c.tag, 0);
        assert_eq!(c.to_u32(), 0x00000000);
    }

    #[test]
    fn test_packed_color_indexed() {
        let c0 = PackedColor::indexed(0);
        assert_eq!(c0.tag, 1);
        assert_eq!(c0.r, 0);

        let c255 = PackedColor::indexed(255);
        assert_eq!(c255.tag, 1);
        assert_eq!(c255.r, 255);
    }

    #[test]
    fn test_packed_color_rgb() {
        let black = PackedColor::rgb(0, 0, 0);
        assert_eq!(black.tag, 2);
        assert_eq!((black.r, black.g, black.b), (0, 0, 0));

        let white = PackedColor::rgb(255, 255, 255);
        assert_eq!(white.tag, 2);
        assert_eq!((white.r, white.g, white.b), (255, 255, 255));
    }

    #[test]
    fn test_packed_color_u32_roundtrip() {
        let colors = [
            PackedColor::DEFAULT,
            PackedColor::indexed(42),
            PackedColor::rgb(100, 200, 50),
        ];
        for c in colors {
            assert_eq!(PackedColor::from_u32(c.to_u32()), c);
        }
    }

    // ── Cell tests ───────────────────────────────────────

    #[test]
    fn test_cell_size() {
        assert_eq!(std::mem::size_of::<Cell>(), 34);
    }

    #[test]
    fn test_cell_ascii() {
        let mut cell = Cell::EMPTY;
        cell.set_char("A");
        cell.width = 1;
        assert_eq!(cell.get_char_inline(), Some("A"));
        assert_eq!(cell.width, 1);
        assert!(!cell.is_overflow());
    }

    #[test]
    fn test_cell_cjk() {
        let mut cell = Cell::EMPTY;
        cell.set_char("漢");
        cell.width = 2;
        assert_eq!(cell.get_char_inline(), Some("漢"));
        assert_eq!(cell.width, 2);
    }

    #[test]
    fn test_cell_emoji() {
        let mut cell = Cell::EMPTY;
        // Flag emoji (8 bytes UTF-8)
        cell.set_char("🇯🇵");
        cell.width = 2;
        assert_eq!(cell.get_char_inline(), Some("🇯🇵"));
    }

    #[test]
    fn test_cell_overflow() {
        let mut cell = Cell::EMPTY;
        // Create a string > 16 bytes
        let long = "👨‍👩‍👧‍👦"; // ZWJ family emoji, >16 bytes
        let bytes_len = long.as_bytes().len();
        assert!(
            bytes_len > 16,
            "Test string must exceed 16 bytes, got {bytes_len}"
        );
        cell.set_char(long);
        assert!(cell.is_overflow());
        assert_eq!(cell.get_char_inline(), None);
    }

    #[test]
    fn test_cell_empty() {
        let cell = Cell::EMPTY;
        assert_eq!(cell.get_char_inline(), Some(" "));
        assert_eq!(cell.width, 1);
        assert_eq!(cell.fg, PackedColor::DEFAULT);
        assert_eq!(cell.bg, PackedColor::DEFAULT);
        assert_eq!(cell.flags, 0);
    }

    // ── Style flags tests ────────────────────────────────

    #[test]
    fn test_style_flags_individual() {
        let flags = [
            (STYLE_BOLD, "bold"),
            (STYLE_DIM, "dim"),
            (STYLE_ITALIC, "italic"),
            (STYLE_UNDERLINE, "underline"),
            (STYLE_BLINK, "blink"),
            (STYLE_REVERSE, "reverse"),
            (STYLE_HIDDEN, "hidden"),
            (STYLE_STRIKETHROUGH, "strikethrough"),
        ];
        for (flag, name) in flags {
            assert_ne!(flag, 0, "{name} should be non-zero");
            // Check each flag sets exactly one bit
            assert_eq!(
                flag.count_ones(),
                1,
                "{name} should have exactly one bit set"
            );
        }
    }

    #[test]
    fn test_style_flags_all_combined() {
        let all = STYLE_BOLD
            | STYLE_DIM
            | STYLE_ITALIC
            | STYLE_UNDERLINE
            | STYLE_BLINK
            | STYLE_REVERSE
            | STYLE_HIDDEN
            | STYLE_STRIKETHROUGH;
        assert_eq!(all, 0x00FF);
    }

    // ── Overflow table tests ─────────────────────────────

    #[test]
    fn test_overflow_clear_row() {
        let mut table = OverflowTable::new();
        table.insert((0, 5), "a".to_string());
        table.insert((3, 5), "b".to_string());
        table.insert((0, 6), "c".to_string());

        overflow_clear_row(&mut table, 5);

        assert!(table.get(&(0, 5)).is_none());
        assert!(table.get(&(3, 5)).is_none());
        assert_eq!(table.get(&(0, 6)), Some(&"c".to_string()));
    }

    // ── Reverse index tests ─────────────────────────────

    #[test]
    fn test_ridx_insert_and_remove() {
        let mut ridx = OverflowRowIndex::new();
        overflow_ridx_insert(&mut ridx, 5, 10);
        overflow_ridx_insert(&mut ridx, 5, 20);
        overflow_ridx_insert(&mut ridx, 5, 10); // duplicate: no-op
        assert_eq!(ridx.get(&5).unwrap().len(), 2);

        overflow_ridx_remove(&mut ridx, 5, 10);
        assert_eq!(ridx.get(&5).unwrap(), &vec![20]);

        overflow_ridx_remove(&mut ridx, 5, 20);
        assert!(!ridx.contains_key(&5)); // row removed entirely
    }

    #[test]
    fn test_ridx_clear_row() {
        let mut ridx = OverflowRowIndex::new();
        overflow_ridx_insert(&mut ridx, 3, 1);
        overflow_ridx_insert(&mut ridx, 3, 5);
        overflow_ridx_insert(&mut ridx, 4, 2);

        overflow_ridx_clear_row(&mut ridx, 3);
        assert!(!ridx.contains_key(&3));
        assert!(ridx.contains_key(&4));
    }

    #[test]
    fn test_ridx_clear_range() {
        let mut ridx = OverflowRowIndex::new();
        overflow_ridx_insert(&mut ridx, 5, 0);
        overflow_ridx_insert(&mut ridx, 5, 5);
        overflow_ridx_insert(&mut ridx, 5, 10);

        overflow_ridx_clear_range(&mut ridx, 5, 3, 8);
        // Only col 5 is in [3,8) range, so removed
        let cols = ridx.get(&5).unwrap();
        assert_eq!(cols.len(), 2);
        assert!(cols.contains(&0));
        assert!(cols.contains(&10));
    }

    #[test]
    fn test_ridx_rebuild() {
        let mut table = OverflowTable::new();
        table.insert((1, 10), "a".to_string());
        table.insert((5, 10), "b".to_string());
        table.insert((3, 20), "c".to_string());

        let ridx = overflow_ridx_rebuild(&table);
        assert_eq!(ridx.get(&10).unwrap().len(), 2);
        assert_eq!(ridx.get(&20).unwrap().len(), 1);
    }
}
