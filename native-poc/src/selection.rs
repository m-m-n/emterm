//! Selection state + resolution.
//!
//! PoC scope: line-based selection only (no rectangular selection). The
//! selection is described by an anchor and an extent, both in cell
//! coordinates. `contains` answers "is this cell currently selected?", and
//! `resolve` walks the terminal core to produce the text string for
//! clipboard copy.
//!
//! Phase 6: `resolve` reads cells through `term_core::TerminalCore` instead
//! of the Phase 1 PoC's bespoke `Grid` type.

use term_core::terminal_core::TerminalCore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub row: u16,
    pub col: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub extent: Pos,
}

impl Selection {
    pub fn new(anchor: Pos) -> Self {
        Self {
            anchor,
            extent: anchor,
        }
    }

    /// Compute the normalized (start, end) where `start <= end` in
    /// reading order.
    pub fn ordered(&self) -> (Pos, Pos) {
        let (a, b) = (self.anchor, self.extent);
        if (a.row, a.col) <= (b.row, b.col) {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// Is `(row, col)` inside the selection (inclusive of both endpoints)?
    pub fn contains(&self, row: u16, col: u16) -> bool {
        let (start, end) = self.ordered();
        if row < start.row || row > end.row {
            return false;
        }
        if start.row == end.row {
            return col >= start.col && col <= end.col;
        }
        if row == start.row {
            return col >= start.col;
        }
        if row == end.row {
            return col <= end.col;
        }
        true
    }

    /// Resolve the selection against a terminal core into a plain text
    /// string. Lines are joined with `\n`. Trailing whitespace on each
    /// line is trimmed (typical terminal copy behavior).
    pub fn resolve(&self, core: &TerminalCore) -> String {
        let (start, end) = self.ordered();
        let cols = core.cols();
        let rows = core.rows();
        if rows == 0 || cols == 0 {
            return String::new();
        }
        let mut out = String::new();
        for row in start.row..=end.row.min(rows - 1) {
            let (c0, c1) = if start.row == end.row {
                (start.col, end.col.min(cols - 1))
            } else if row == start.row {
                (start.col, cols - 1)
            } else if row == end.row {
                (0, end.col.min(cols - 1))
            } else {
                (0, cols - 1)
            };
            let mut line = String::new();
            for col in c0..=c1 {
                let ch = core.get_cell_char(col, row);
                if !ch.is_empty() {
                    line.push_str(&ch);
                }
            }
            // Trim trailing spaces (terminal copy convention).
            let trimmed = line.trim_end_matches(' ').to_string();
            out.push_str(&trimmed);
            if row != end.row {
                out.push('\n');
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_core::terminal_core::TerminalCore;

    fn build_core(input: &[&[u8]]) -> TerminalCore {
        let mut core = TerminalCore::new(10, 3, 100);
        for chunk in input {
            core.process_pty_data(chunk);
        }
        core
    }

    #[test]
    fn single_row_selection_resolves() {
        let core = build_core(&[b"hello"]);
        let sel = Selection {
            anchor: Pos { row: 0, col: 0 },
            extent: Pos { row: 0, col: 4 },
        };
        assert_eq!(sel.resolve(&core), "hello");
    }

    #[test]
    fn multi_row_selection_joins_with_newline() {
        let core = build_core(&[b"ab\r\ncd\r\nef"]);
        let sel = Selection {
            anchor: Pos { row: 0, col: 0 },
            extent: Pos { row: 2, col: 1 },
        };
        assert_eq!(sel.resolve(&core), "ab\ncd\nef");
    }

    #[test]
    fn selection_handles_reverse_anchor_extent() {
        let core = build_core(&[b"hello"]);
        let sel = Selection {
            anchor: Pos { row: 0, col: 4 },
            extent: Pos { row: 0, col: 0 },
        };
        assert_eq!(sel.resolve(&core), "hello");
    }

    #[test]
    fn trailing_spaces_trimmed() {
        let mut core = TerminalCore::new(10, 1, 10);
        core.process_pty_data(b"hi");
        let sel = Selection {
            anchor: Pos { row: 0, col: 0 },
            extent: Pos { row: 0, col: 9 },
        };
        assert_eq!(sel.resolve(&core), "hi");
    }

    #[test]
    fn contains_handles_endpoints() {
        let sel = Selection {
            anchor: Pos { row: 0, col: 2 },
            extent: Pos { row: 1, col: 3 },
        };
        assert!(sel.contains(0, 2));
        assert!(sel.contains(1, 3));
        assert!(sel.contains(0, 9)); // anything after start on row 0
        assert!(sel.contains(1, 0));
        assert!(!sel.contains(0, 1));
        assert!(!sel.contains(2, 0));
    }
}
