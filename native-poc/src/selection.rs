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
//!
//! Phase 4 sub-phase 4 adds:
//! - `SelectionMode { Character, Word, Line }` so double / triple click can
//!   snap the resolved range to a word or full line, matching xterm / VTE.
//! - `Selection::extend(pos, core)` which keeps the anchor pinned and walks
//!   the extent, snapping to word or line boundaries when the mode is not
//!   `Character`.
//! - `sanitize_bracket_sequences` + `bracketed_paste` helpers used by the
//!   paste path. A pasted body must not contain `\e[201~` (would otherwise
//!   close the bracketed paste prematurely, letting the inner content escape
//!   into command interpretation).

use term_core::terminal_core::TerminalCore;

/// Click-count-derived selection grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// Free per-cell range (single click + drag).
    #[default]
    Character,
    /// Snap both endpoints to the word containing the anchor / extent.
    Word,
    /// Snap to the whole row(s).
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub row: u16,
    pub col: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub extent: Pos,
    pub mode: SelectionMode,
}

impl Selection {
    pub fn new(anchor: Pos) -> Self {
        Self {
            anchor,
            extent: anchor,
            mode: SelectionMode::Character,
        }
    }

    pub fn new_with_mode(anchor: Pos, mode: SelectionMode) -> Self {
        Self {
            anchor,
            extent: anchor,
            mode,
        }
    }

    /// Compute the normalized (start, end) where `start <= end` in
    /// reading order. Honors `mode`: word selections expand each endpoint
    /// to its word boundary; line selections cover whole rows.
    pub fn ordered(&self) -> (Pos, Pos) {
        let (a, b) = (self.anchor, self.extent);
        if (a.row, a.col) <= (b.row, b.col) {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// Move the moving endpoint (the one that is not the anchor) to `pos`.
    /// For `Word` / `Line` modes the endpoint snaps to the relevant
    /// boundary against `core`.
    pub fn extend(&mut self, pos: Pos, core: &TerminalCore) {
        self.extent = pos;
        match self.mode {
            SelectionMode::Character => {}
            SelectionMode::Word => {
                let (anchor_word_start, anchor_word_end) =
                    word_boundary(core, self.anchor.row, self.anchor.col);
                let (extent_word_start, extent_word_end) =
                    word_boundary(core, self.extent.row, self.extent.col);
                // Pick the outermost edges so the range covers both words.
                let (start_row, start_col, end_row, end_col) =
                    if (self.anchor.row, self.anchor.col) <= (self.extent.row, self.extent.col) {
                        (
                            self.anchor.row,
                            anchor_word_start,
                            self.extent.row,
                            extent_word_end,
                        )
                    } else {
                        (
                            self.extent.row,
                            extent_word_start,
                            self.anchor.row,
                            anchor_word_end,
                        )
                    };
                self.anchor = Pos {
                    row: start_row,
                    col: start_col,
                };
                self.extent = Pos {
                    row: end_row,
                    col: end_col,
                };
            }
            SelectionMode::Line => {
                let cols = core.cols().saturating_sub(1);
                let (a_row, e_row) = if self.anchor.row <= self.extent.row {
                    (self.anchor.row, self.extent.row)
                } else {
                    (self.extent.row, self.anchor.row)
                };
                self.anchor = Pos { row: a_row, col: 0 };
                self.extent = Pos {
                    row: e_row,
                    col: cols,
                };
            }
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

/// Word boundary classifier. ASCII alnum + `_` are word chars; everything
/// else (including non-ASCII) defers to `char::is_alphanumeric`. Whitespace
/// and an empty cell terminate a word.
fn is_word_char(ch: &str) -> bool {
    if ch.is_empty() {
        return false;
    }
    let mut chars = ch.chars();
    let c = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if chars.next().is_some() {
        // Multi-codepoint cluster (emoji ZWJ, etc.) — treat as word.
        return !c.is_whitespace();
    }
    c == '_' || c.is_alphanumeric()
}

/// Find the (start_col, end_col) of the contiguous word covering
/// (`row`, `col`) on a single row. If the cell at the cursor is not a
/// word character, the range collapses to a single cell.
pub(crate) fn word_boundary(core: &TerminalCore, row: u16, col: u16) -> (u16, u16) {
    let cols = core.cols();
    if cols == 0 {
        return (0, 0);
    }
    let col = col.min(cols - 1);
    let ch = core.get_cell_char(col, row);
    if !is_word_char(&ch) {
        return (col, col);
    }
    let mut start = col;
    while start > 0 {
        let prev = core.get_cell_char(start - 1, row);
        if !is_word_char(&prev) {
            break;
        }
        start -= 1;
    }
    let mut end = col;
    while end + 1 < cols {
        let next = core.get_cell_char(end + 1, row);
        if !is_word_char(&next) {
            break;
        }
        end += 1;
    }
    (start, end)
}

/// Strip embedded bracketed-paste end markers from the body so a malicious
/// paste cannot terminate the wrapping prematurely. Replaces every
/// occurrence of `\e[201~` with the empty string.
pub fn sanitize_bracket_sequences(text: &str) -> String {
    text.replace("\x1b[201~", "")
}

/// Wrap `text` for transmission to the PTY. When `enabled` is true, the
/// payload is `ESC [ 200 ~ <sanitized body> ESC [ 201 ~`. Otherwise the
/// sanitized body is returned as-is. (We still sanitize even when
/// bracketing is off, so a pasted bracket-end marker never reaches the
/// shell as an injected control.)
pub fn bracketed_paste(text: &str, enabled: bool) -> String {
    let body = sanitize_bracket_sequences(text);
    if enabled {
        let mut out = String::with_capacity(body.len() + 12);
        out.push_str("\x1b[200~");
        out.push_str(&body);
        out.push_str("\x1b[201~");
        out
    } else {
        body
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

    fn build_wide_core(cols: u16, input: &[&[u8]]) -> TerminalCore {
        let mut core = TerminalCore::new(cols, 3, 100);
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
            mode: SelectionMode::Character,
        };
        assert_eq!(sel.resolve(&core), "hello");
    }

    #[test]
    fn multi_row_selection_joins_with_newline() {
        let core = build_core(&[b"ab\r\ncd\r\nef"]);
        let sel = Selection {
            anchor: Pos { row: 0, col: 0 },
            extent: Pos { row: 2, col: 1 },
            mode: SelectionMode::Character,
        };
        assert_eq!(sel.resolve(&core), "ab\ncd\nef");
    }

    #[test]
    fn selection_handles_reverse_anchor_extent() {
        let core = build_core(&[b"hello"]);
        let sel = Selection {
            anchor: Pos { row: 0, col: 4 },
            extent: Pos { row: 0, col: 0 },
            mode: SelectionMode::Character,
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
            mode: SelectionMode::Character,
        };
        assert_eq!(sel.resolve(&core), "hi");
    }

    #[test]
    fn contains_handles_endpoints() {
        let sel = Selection {
            anchor: Pos { row: 0, col: 2 },
            extent: Pos { row: 1, col: 3 },
            mode: SelectionMode::Character,
        };
        assert!(sel.contains(0, 2));
        assert!(sel.contains(1, 3));
        assert!(sel.contains(0, 9)); // anything after start on row 0
        assert!(sel.contains(1, 0));
        assert!(!sel.contains(0, 1));
        assert!(!sel.contains(2, 0));
    }

    #[test]
    fn word_boundary_finds_alnum_run() {
        let core = build_core(&[b"foo bar"]);
        // Cursor in the middle of "foo" → covers 0..=2.
        assert_eq!(word_boundary(&core, 0, 1), (0, 2));
        // Cursor on the space → collapses to that cell.
        assert_eq!(word_boundary(&core, 0, 3), (3, 3));
        // Cursor in "bar" → covers 4..=6.
        assert_eq!(word_boundary(&core, 0, 5), (4, 6));
    }

    #[test]
    fn word_boundary_includes_underscore() {
        let core = build_core(&[b"foo_bar baz"]);
        // Underscore is a word character.
        assert_eq!(word_boundary(&core, 0, 3), (0, 6));
    }

    #[test]
    fn word_boundary_collapses_on_empty_cell() {
        let core = build_core(&[b"hi"]);
        // Column 5 is past the written text — empty cells aren't words.
        assert_eq!(word_boundary(&core, 0, 5), (5, 5));
    }

    #[test]
    fn extend_in_character_mode_moves_extent_only() {
        let core = build_core(&[b"hello world"]);
        let mut sel = Selection::new(Pos { row: 0, col: 2 });
        sel.extend(Pos { row: 0, col: 6 }, &core);
        assert_eq!(sel.anchor, Pos { row: 0, col: 2 });
        assert_eq!(sel.extent, Pos { row: 0, col: 6 });
    }

    #[test]
    fn extend_in_word_mode_snaps_to_word_boundaries() {
        // 20-cell row so "foo bar baz" doesn't wrap.
        let core = build_wide_core(20, &[b"foo bar baz"]);
        // Anchor inside "foo", extent inside "baz".
        let mut sel = Selection::new_with_mode(Pos { row: 0, col: 1 }, SelectionMode::Word);
        sel.extend(Pos { row: 0, col: 9 }, &core);
        // Range should cover the outer edges: 0 ("f" in foo) → 10 ("z" in baz).
        let (start, end) = sel.ordered();
        assert_eq!(start, Pos { row: 0, col: 0 });
        assert_eq!(end, Pos { row: 0, col: 10 });
    }

    #[test]
    fn extend_in_word_mode_handles_reverse_drag() {
        let core = build_wide_core(20, &[b"foo bar baz"]);
        // Anchor in "baz", drag back to "foo".
        let mut sel = Selection::new_with_mode(Pos { row: 0, col: 9 }, SelectionMode::Word);
        sel.extend(Pos { row: 0, col: 1 }, &core);
        let (start, end) = sel.ordered();
        assert_eq!(start, Pos { row: 0, col: 0 });
        assert_eq!(end, Pos { row: 0, col: 10 });
    }

    #[test]
    fn extend_in_line_mode_covers_full_rows() {
        let core = build_core(&[b"row0\r\nrow1\r\nrow2"]);
        let mut sel = Selection::new_with_mode(Pos { row: 0, col: 3 }, SelectionMode::Line);
        sel.extend(Pos { row: 2, col: 1 }, &core);
        let (start, end) = sel.ordered();
        assert_eq!(start.col, 0);
        assert_eq!(end.col, core.cols() - 1);
        assert_eq!(start.row, 0);
        assert_eq!(end.row, 2);
    }

    #[test]
    fn sanitize_strips_embedded_bracket_end() {
        assert_eq!(sanitize_bracket_sequences("abc\x1b[201~def"), "abcdef");
        // Multiple occurrences.
        assert_eq!(
            sanitize_bracket_sequences("\x1b[201~a\x1b[201~b\x1b[201~"),
            "ab"
        );
        // No occurrence: untouched.
        assert_eq!(sanitize_bracket_sequences("plain"), "plain");
    }

    #[test]
    fn bracketed_paste_wraps_when_enabled() {
        let got = bracketed_paste("hello", true);
        assert_eq!(got, "\x1b[200~hello\x1b[201~");
    }

    #[test]
    fn bracketed_paste_passthrough_when_disabled() {
        let got = bracketed_paste("hello", false);
        assert_eq!(got, "hello");
    }

    #[test]
    fn bracketed_paste_sanitizes_body() {
        // Embedded end marker is stripped before the wrap, so the inner
        // text cannot terminate the bracketed paste prematurely.
        let got = bracketed_paste("evil\x1b[201~payload", true);
        assert_eq!(got, "\x1b[200~evilpayload\x1b[201~");
        // Even when disabled, the sanitization still applies (defense in depth).
        let got2 = bracketed_paste("evil\x1b[201~payload", false);
        assert_eq!(got2, "evilpayload");
    }
}
