//! Compressed cell record used in scrollback storage.
//!
//! The active viewport keeps the original 34-byte [`Cell`]; lines that have
//! been evicted from the viewport are converted to [`SlimCell`] (8 bytes) to
//! reduce memory usage.
//!
//! Style attributes are deduplicated through a [`StyleTable`](crate::style_table::StyleTable)
//! and graphemes that do not fit inline are deduplicated through a
//! [`CharTable`](crate::char_table::CharTable).
//!
//! Layout (`#[repr(C)]`, total = 8 bytes):
//!   - `char_ref:  u32` — packed UTF-8 bytes (INLINE_ASCII) or `CharTable` id (CHAR_TABLE)
//!   - `width:     u8`  — display width (0 for wide-cont, 1 normal, 2 wide)
//!   - `flags:     u8`  — see `SLIM_FLAG_*` constants below
//!   - `style_id:  u16` — `StyleTable` id (id 0 = default)

use crate::cell::Cell;
use crate::char_table::CharTable;
use crate::style_table::{StyleEntry, StyleTable};

/// `char_ref` holds the inline UTF-8 bytes of a grapheme up to 4 bytes.
pub const SLIM_FLAG_INLINE_ASCII: u8 = 0x01;
/// `char_ref` holds a `CharTable` id (u32) for graphemes longer than 4 bytes.
pub const SLIM_FLAG_CHAR_TABLE: u8 = 0x02;
/// Right half of a double-width cell. `char_ref` is unused.
pub const SLIM_FLAG_WIDE_CONT: u8 = 0x04;
/// Mask for the inline-byte-length field in `flags` (bits 4-5, valid only when
/// `SLIM_FLAG_INLINE_ASCII` is set). Stored as `(len - 1)` so values 0..=3
/// represent UTF-8 lengths 1..=4. Explicit length lets us round-trip cells that
/// contain NUL (`\0`) bytes losslessly.
pub const SLIM_INLINE_LEN_MASK: u8 = 0x30;
pub const SLIM_INLINE_LEN_SHIFT: u8 = 4;

/// Compressed cell for scrollback storage. Exactly 8 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct SlimCell {
    pub char_ref: u32,
    pub width: u8,
    pub flags: u8,
    pub style_id: u16,
}

impl SlimCell {
    /// A blank cell with the default style and a single-space inline char.
    #[allow(dead_code)]
    pub const EMPTY: Self = Self {
        // ASCII space ' ' (0x20) packed little-endian into a u32.
        char_ref: 0x20,
        width: 1,
        flags: SLIM_FLAG_INLINE_ASCII,
        style_id: 0,
    };

    #[inline]
    pub fn is_inline_ascii(&self) -> bool {
        self.flags & SLIM_FLAG_INLINE_ASCII != 0
    }

    #[inline]
    pub fn is_char_table(&self) -> bool {
        self.flags & SLIM_FLAG_CHAR_TABLE != 0
    }

    #[inline]
    pub fn is_wide_cont(&self) -> bool {
        self.flags & SLIM_FLAG_WIDE_CONT != 0
    }
}

// ── Compression bridge ───────────────────────────────────

/// Compress a [`Cell`] into a [`SlimCell`], interning its style and (if
/// necessary) its grapheme into the supplied tables.
///
/// `overflow_str` must be `Some` whenever `cell.is_overflow()` (i.e. the
/// grapheme is stored in the per-(col, row) overflow side table). For inline
/// cells (≤16 bytes) the argument is ignored.
///
/// Refcount side-effects: increments style refcount once and char refcount
/// once when the CharTable is used.
pub fn cell_to_slim(
    cell: &Cell,
    overflow_str: Option<&str>,
    styles: &mut StyleTable,
    chars: &mut CharTable,
) -> SlimCell {
    let style_id = styles.intern(StyleEntry {
        fg: cell.fg,
        bg: cell.bg,
        flags: cell.flags,
        underline_style: cell.underline_style,
        underline_color: cell.underline_color,
        hyperlink_id: cell.hyperlink_id,
    });

    // Width 0 = right half of a wide cell (continuation marker).
    if cell.width == 0 {
        return SlimCell {
            char_ref: 0,
            width: 0,
            flags: SLIM_FLAG_WIDE_CONT,
            style_id,
        };
    }

    if cell.is_overflow() {
        let s = overflow_str.unwrap_or("?");
        let id = chars.intern(s);
        return SlimCell {
            char_ref: id,
            width: cell.width,
            flags: SLIM_FLAG_CHAR_TABLE,
            style_id,
        };
    }

    let len = cell.char_len as usize;
    debug_assert!(len <= 16, "non-overflow cell must have char_len <= 16");
    let bytes = &cell.char_data[..len];

    if (1..=4).contains(&len) {
        // Inline UTF-8 (1..=4 bytes) — pack into char_ref little-endian and
        // record the length explicitly in the flags' length field so that
        // graphemes containing a NUL byte round-trip correctly.
        let mut buf = [0u8; 4];
        buf[..len].copy_from_slice(bytes);
        let len_bits = ((len - 1) as u8) << SLIM_INLINE_LEN_SHIFT;
        SlimCell {
            char_ref: u32::from_le_bytes(buf),
            width: cell.width,
            flags: SLIM_FLAG_INLINE_ASCII | len_bits,
            style_id,
        }
    } else {
        // 5..=16 bytes — needs CharTable.
        let s = std::str::from_utf8(bytes).unwrap_or("?");
        let id = chars.intern(s);
        SlimCell {
            char_ref: id,
            width: cell.width,
            flags: SLIM_FLAG_CHAR_TABLE,
            style_id,
        }
    }
}

/// Decompress a [`SlimCell`] back into a [`Cell`].
///
/// Does **not** modify refcounts. If the underlying string from CharTable is
/// longer than 16 bytes the resulting Cell is marked overflow (`char_len ==
/// 0xFF`) and the caller is expected to retrieve the original string via
/// [`slim_to_cell_overflow_str`] for storage in its own overflow side table.
pub fn slim_to_cell(slim: &SlimCell, styles: &StyleTable, chars: &CharTable) -> Cell {
    let style = styles.get_or_default(slim.style_id);
    let mut cell = Cell::EMPTY;
    cell.width = slim.width;
    cell.fg = style.fg;
    cell.bg = style.bg;
    cell.flags = style.flags;
    cell.underline_style = style.underline_style;
    cell.underline_color = style.underline_color;
    cell.hyperlink_id = style.hyperlink_id;

    if slim.is_wide_cont() {
        cell.char_data = [0; 16];
        cell.char_len = 0;
        return cell;
    }

    if slim.is_inline_ascii() {
        let bytes = slim.char_ref.to_le_bytes();
        // Length is stored explicitly in flags bits 4-5 as (len - 1).
        let len = ((slim.flags & SLIM_INLINE_LEN_MASK) >> SLIM_INLINE_LEN_SHIFT) as usize + 1;
        debug_assert!((1..=4).contains(&len));
        cell.char_data[..len].copy_from_slice(&bytes[..len]);
        cell.char_len = len as u8;
        return cell;
    }

    if slim.is_char_table() {
        let s = chars.get_or_default(slim.char_ref);
        let bytes = s.as_bytes();
        if bytes.len() <= 16 {
            cell.char_data[..bytes.len()].copy_from_slice(bytes);
            cell.char_len = bytes.len() as u8;
        } else {
            // Overflow: caller must resolve via CharTable themselves.
            cell.char_data = [0; 16];
            cell.char_len = 0xFF;
        }
        return cell;
    }

    // Unknown flag combination — fall back to a blank space.
    cell.char_data[0] = b' ';
    cell.char_len = 1;
    cell
}

/// For a `SlimCell` that uses the CharTable, return the underlying string for
/// overflow handling. Returns an empty string if the slim cell is not in
/// CharTable mode.
pub fn slim_overflow_str<'a>(slim: &SlimCell, chars: &'a CharTable) -> &'a str {
    if slim.is_char_table() {
        chars.get_or_default(slim.char_ref)
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{Cell, PackedColor, STYLE_BOLD};

    #[test]
    fn slim_cell_is_8_bytes() {
        assert_eq!(std::mem::size_of::<SlimCell>(), 8);
    }

    #[test]
    fn slim_cell_alignment_matches_layout() {
        // repr(C) field layout is char_ref (4), width (1), flags (1), style_id (2).
        // Sanity: the struct should fit in 8 bytes with natural alignment.
        assert_eq!(std::mem::align_of::<SlimCell>(), 4);
    }

    #[test]
    fn flag_constants_are_distinct_bits() {
        assert_eq!(SLIM_FLAG_INLINE_ASCII.count_ones(), 1);
        assert_eq!(SLIM_FLAG_CHAR_TABLE.count_ones(), 1);
        assert_eq!(SLIM_FLAG_WIDE_CONT.count_ones(), 1);
        assert_eq!(SLIM_FLAG_INLINE_ASCII & SLIM_FLAG_CHAR_TABLE, 0);
        assert_eq!(SLIM_FLAG_INLINE_ASCII & SLIM_FLAG_WIDE_CONT, 0);
        assert_eq!(SLIM_FLAG_CHAR_TABLE & SLIM_FLAG_WIDE_CONT, 0);
    }

    fn make_styled_cell(s: &str, width: u8, flags: u16) -> Cell {
        let mut cell = Cell::EMPTY;
        cell.set_char(s);
        cell.width = width;
        cell.flags = flags;
        cell.fg = PackedColor::rgb(255, 100, 50);
        cell.bg = PackedColor::indexed(4);
        cell.underline_style = 2;
        cell.underline_color = [10, 20, 30];
        cell.hyperlink_id = 7;
        cell
    }

    #[test]
    fn round_trip_ascii() {
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let cell = make_styled_cell("A", 1, STYLE_BOLD);
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        assert!(slim.is_inline_ascii());
        let back = slim_to_cell(&slim, &styles, &chars);
        assert_eq!(back.get_char_inline(), Some("A"));
        assert_eq!(back.width, 1);
        assert_eq!(back.flags, STYLE_BOLD);
        assert_eq!(back.fg, cell.fg);
        assert_eq!(back.bg, cell.bg);
        assert_eq!(back.underline_style, 2);
        assert_eq!(back.underline_color, [10, 20, 30]);
        assert_eq!(back.hyperlink_id, 7);
        // Char table stays empty for inline ASCII cells.
        assert_eq!(chars.live_entries(), 0);
    }

    #[test]
    fn round_trip_3byte_inline() {
        // CJK 漢 = 3 bytes UTF-8 → fits in u32 inline.
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let cell = make_styled_cell("漢", 2, 0);
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        assert!(slim.is_inline_ascii(), "3-byte chars stay inline");
        let back = slim_to_cell(&slim, &styles, &chars);
        assert_eq!(back.get_char_inline(), Some("漢"));
        assert_eq!(back.width, 2);
        assert_eq!(chars.live_entries(), 0);
    }

    #[test]
    fn round_trip_4byte_inline() {
        // Single-codepoint emoji 😀 = 4 bytes UTF-8 → exactly fits.
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let cell = make_styled_cell("😀", 2, 0);
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        assert!(slim.is_inline_ascii());
        let back = slim_to_cell(&slim, &styles, &chars);
        assert_eq!(back.get_char_inline(), Some("😀"));
        assert_eq!(chars.live_entries(), 0);
    }

    #[test]
    fn round_trip_8byte_chartable() {
        // Flag emoji 🇯🇵 = 8 bytes → goes through CharTable.
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let cell = make_styled_cell("🇯🇵", 2, 0);
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        assert!(slim.is_char_table());
        let back = slim_to_cell(&slim, &styles, &chars);
        assert_eq!(back.get_char_inline(), Some("🇯🇵"));
        assert_eq!(chars.live_entries(), 1);
    }

    #[test]
    fn round_trip_zwj_overflow() {
        // ZWJ family emoji 👨‍👩‍👧‍👦 is 25 bytes — Cell stores as overflow,
        // SlimCell uses CharTable.
        let zwj = "👨‍👩‍👧‍👦";
        assert!(zwj.as_bytes().len() > 16);
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let mut cell = Cell::EMPTY;
        cell.set_char(zwj);
        cell.width = 2;
        assert!(cell.is_overflow());
        let slim = cell_to_slim(&cell, Some(zwj), &mut styles, &mut chars);
        assert!(slim.is_char_table());
        let back = slim_to_cell(&slim, &styles, &chars);
        // back should be marked overflow and the original string can be fetched.
        assert!(back.is_overflow());
        assert_eq!(slim_overflow_str(&slim, &chars), zwj);
    }

    #[test]
    fn round_trip_wide_cont() {
        // Width-0 continuation cell.
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let mut cell = Cell::EMPTY;
        cell.width = 0;
        cell.char_len = 0;
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        assert!(slim.is_wide_cont());
        assert_eq!(slim.width, 0);
        let back = slim_to_cell(&slim, &styles, &chars);
        assert_eq!(back.width, 0);
        // No char interning happened.
        assert_eq!(chars.live_entries(), 0);
    }

    #[test]
    fn round_trip_inline_with_nul_byte() {
        // A grapheme that contains a NUL (0x00) must round-trip without being
        // truncated by the decoder. Previously the decoder inferred length by
        // scanning for the first 0 byte; the explicit length field in flags
        // makes this lossless.
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let mut cell = Cell::EMPTY;
        // 4-byte payload with an embedded NUL: [0x41, 0x00, 0x42, 0x43]
        // (not valid UTF-8 as a grapheme, but the SlimCell layer must not
        // silently mangle the bytes; UTF-8 validation happens elsewhere).
        cell.char_data[..4].copy_from_slice(&[0x41, 0x00, 0x42, 0x43]);
        cell.char_len = 4;
        cell.width = 1;
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        assert!(slim.is_inline_ascii());
        let back = slim_to_cell(&slim, &styles, &chars);
        assert_eq!(back.char_len, 4);
        assert_eq!(&back.char_data[..4], &[0x41, 0x00, 0x42, 0x43]);
    }

    #[test]
    fn round_trip_single_nul_byte() {
        // A single NUL byte should also survive a round trip (len = 1).
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let mut cell = Cell::EMPTY;
        cell.char_data[0] = 0x00;
        cell.char_len = 1;
        cell.width = 1;
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        assert!(slim.is_inline_ascii());
        let back = slim_to_cell(&slim, &styles, &chars);
        assert_eq!(back.char_len, 1);
        assert_eq!(back.char_data[0], 0x00);
    }

    #[test]
    fn slim_to_cell_does_not_change_refcounts() {
        let mut styles = StyleTable::new();
        let mut chars = CharTable::new();
        let cell = make_styled_cell("🇯🇵", 2, 0);
        let slim = cell_to_slim(&cell, None, &mut styles, &mut chars);
        let style_count_before = styles.live_entries();
        let char_count_before = chars.live_entries();
        for _ in 0..1000 {
            let _ = slim_to_cell(&slim, &styles, &chars);
        }
        assert_eq!(styles.live_entries(), style_count_before);
        assert_eq!(chars.live_entries(), char_count_before);
    }
}
