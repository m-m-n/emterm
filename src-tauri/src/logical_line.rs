//! Shared soft-wrap logical-line model for link detection and search.
//!
//! Both [`crate::links`] (URL / file-path hover detection over the
//! viewport) and [`crate::search`] (incremental search over scrollback +
//! viewport) need the same machinery: join soft-wrapped physical rows
//! into one logical line, drop the width-0 continuation halves of wide
//! glyphs, keep a per-`char` map back to the physical `(row, col)` it came
//! from, and collapse a regex match's char range into physical cell
//! segments (one per physical row it touches). This module owns that core
//! so the two consumers do not maintain divergent copies.
//!
//! The physical row coordinate is generic (`R`): [`crate::links`] uses the
//! viewport row index (`u16`), while [`crate::search`] uses an absolute
//! scrollback+viewport row index (`u32`). Everything else — the wide-glyph
//! skip, the empty-cell → space substitution, the per-char grapheme
//! mapping, and the range→segment collapse — is identical and lives here.

/// One `char` of a logical line, paired with its physical origin. A
/// multi-`char` grapheme (emoji ZWJ, combining marks) emits one
/// `LogicalCell` per `char`, all sharing the same physical `(row, col)` /
/// `width`, so `cells[i]` stays aligned with the i-th `char` of `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalCell<R> {
    pub row: R,
    pub col: u16,
    /// Physical width in cells (1 normal, 2 wide). A matched span extends
    /// `col_end` by this so a wide glyph covers both of its columns.
    pub width: u16,
}

/// One physical highlight segment: an inclusive-exclusive column span on a
/// single physical row (`col_start <= col < col_end`). A match / link that
/// crosses a soft-wrap boundary yields one segment per physical row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment<R> {
    pub row: R,
    pub col_start: u16,
    pub col_end: u16,
}

/// A soft-wrap-joined logical line: the concatenated `text` plus the
/// per-`char` physical mapping needed to resolve a regex match's char
/// range back into highlight segments. Generic over the physical row
/// coordinate (`u16` viewport row for links, `u32` absolute row for
/// search).
#[derive(Debug, Clone, Default)]
pub struct LogicalLine<R> {
    pub text: String,
    pub cells: Vec<LogicalCell<R>>,
}

impl<R: Copy + PartialEq> LogicalLine<R> {
    /// An empty logical line.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cells: Vec::new(),
        }
    }

    /// Append one physical row's `(grapheme, physical_width)` cells at row
    /// coordinate `row`. Width-0 cells (wide-glyph continuation halves)
    /// must already be dropped by the caller's decode; empty graphemes are
    /// substituted with a single space so the char→cell map matches the
    /// rendered blank cell. The running column starts at `0` for each
    /// physical row and advances by each cell's width.
    ///
    /// One `LogicalCell` is pushed per `char` of the grapheme so
    /// `cells[i]` stays aligned with the i-th `char` of `text` (a
    /// multi-`char` grapheme shares one physical cell).
    pub fn push_row<'a, I>(&mut self, row: R, cells: I)
    where
        I: IntoIterator<Item = (&'a str, u16)>,
    {
        let mut col: u16 = 0;
        for (ch, width) in cells {
            let ch = if ch.is_empty() { " " } else { ch };
            for _ in ch.chars() {
                self.cells.push(LogicalCell { row, col, width });
            }
            self.text.push_str(ch);
            col = col.saturating_add(width);
        }
    }

    /// Byte offset in `text` of the `n`-th `char` (or `text.len()` when `n`
    /// is past the end). Used to clamp an over-long-line truncation to a
    /// char boundary.
    pub fn byte_offset_of_char(&self, n: usize) -> usize {
        self.text
            .char_indices()
            .nth(n)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len())
    }

    /// Map a physical `(row, col)` to its logical char index within this
    /// line, or `None` if the cell is not part of it (e.g. a width-0
    /// continuation cell that was dropped during build).
    pub fn char_index_at(&self, row: R, col: u16) -> Option<usize> {
        self.cells.iter().position(|c| c.row == row && c.col == col)
    }

    /// Collapse a char range `[start_char, end_char)` into physical cell
    /// segments: one `(row, col_start, col_end)` per physical row the range
    /// touches. Adjacent same-row cells merge into one span; wide glyphs
    /// extend `col_end` by their width. Duplicate cells emitted by a
    /// multi-`char` grapheme coalesce so each row stays a single
    /// contiguous run.
    pub fn char_range_to_segments(&self, start_char: usize, end_char: usize) -> Vec<Segment<R>> {
        let mut segments: Vec<Segment<R>> = Vec::new();
        for c in self
            .cells
            .iter()
            .skip(start_char)
            .take(end_char.saturating_sub(start_char))
        {
            let col_start = c.col;
            let col_end = c.col.saturating_add(c.width.max(1));
            match segments.last_mut() {
                Some(seg) if seg.row == c.row && col_start <= seg.col_end => {
                    seg.col_end = seg.col_end.max(col_end);
                }
                _ => segments.push(Segment {
                    row: c.row,
                    col_start,
                    col_end,
                }),
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_row_maps_chars_to_columns() {
        let mut line: LogicalLine<u16> = LogicalLine::new();
        line.push_row(0, [("h", 1), ("i", 1)]);
        assert_eq!(line.text, "hi");
        assert_eq!(line.cells.len(), 2);
        assert_eq!(
            line.cells[0],
            LogicalCell {
                row: 0,
                col: 0,
                width: 1
            }
        );
        assert_eq!(
            line.cells[1],
            LogicalCell {
                row: 0,
                col: 1,
                width: 1
            }
        );
    }

    #[test]
    fn push_row_wide_glyph_advances_two_columns() {
        let mut line: LogicalLine<u16> = LogicalLine::new();
        line.push_row(0, [("あ", 2), ("x", 1)]);
        // "あ" occupies cols 0-1 (width 2), so "x" lands at col 2.
        assert_eq!(
            line.cells[0],
            LogicalCell {
                row: 0,
                col: 0,
                width: 2
            }
        );
        assert_eq!(
            line.cells[1],
            LogicalCell {
                row: 0,
                col: 2,
                width: 1
            }
        );
    }

    #[test]
    fn push_row_empty_cell_becomes_space() {
        let mut line: LogicalLine<u16> = LogicalLine::new();
        line.push_row(0, [("", 1), ("y", 1)]);
        assert_eq!(line.text, " y");
    }

    #[test]
    fn char_range_to_segments_single_row() {
        let mut line: LogicalLine<u16> = LogicalLine::new();
        line.push_row(0, [("a", 1), ("b", 1), ("c", 1)]);
        let segs = line.char_range_to_segments(0, 2);
        assert_eq!(
            segs,
            vec![Segment {
                row: 0,
                col_start: 0,
                col_end: 2
            }]
        );
    }

    #[test]
    fn char_range_to_segments_spans_two_rows() {
        let mut line: LogicalLine<u16> = LogicalLine::new();
        line.push_row(0, [("a", 1), ("b", 1)]);
        line.push_row(1, [("c", 1), ("d", 1)]);
        // Range covering "b" (row 0) and "c" (row 1) yields one seg per row.
        let segs = line.char_range_to_segments(1, 3);
        assert_eq!(
            segs,
            vec![
                Segment {
                    row: 0,
                    col_start: 1,
                    col_end: 2
                },
                Segment {
                    row: 1,
                    col_start: 0,
                    col_end: 1
                },
            ]
        );
    }

    #[test]
    fn char_index_at_finds_and_misses() {
        let mut line: LogicalLine<u16> = LogicalLine::new();
        line.push_row(0, [("a", 1), ("b", 1)]);
        assert_eq!(line.char_index_at(0, 1), Some(1));
        assert_eq!(line.char_index_at(0, 9), None);
        assert_eq!(line.char_index_at(5, 0), None);
    }
}
