//! URL + file-path link detection over the terminal grid.
//!
//! Faithful port of the WebView build's `src/terminal/url-detector.ts`
//! (detection regexes + trailing-punctuation trim + soft-wrap logical
//! line construction) and the detection half of
//! `src/terminal-app/handlers/link.ts` (CWD resolution + safe-scheme
//! whitelist). The click side (spawning `xdg-open` / the editor command)
//! lives in `window_host.rs`; this module is pure logic so it can be unit
//! tested without a window / event loop.
//!
//! Hover detection runs `find_link_at` whenever the pointer crosses into a
//! new grid cell. The returned [`DetectedLink`] carries the physical cell
//! ranges (one per wrapped physical row) so the renderer can underline the
//! matched span — mirroring the WebView's hover-only underline (no Ctrl
//! required to underline; Ctrl is only needed to *open* the link).

use term_core::terminal_core::TerminalCore;

/// URL pattern matching common protocols. Mirrors `URL_REGEX` in
/// `url-detector.ts` (line 109).
const URL_PATTERN: &str = r#"(?:https?|ftp|file)://[^\s<>"'`)\]},;]+"#;

/// File-path pattern matching paths with a mandatory line number. Mirrors
/// `FILE_PATH_REGEX` in `url-detector.ts` (lines 171-172).
const FILE_PATH_PATTERN: &str =
    r"(?:\.?\./)?/?(?:[a-zA-Z0-9_@.-]+/)*[a-zA-Z0-9_@.-]+\.[a-zA-Z0-9]+:\d+(?::\d+)?";

thread_local! {
    static URL_RE: regex::Regex = regex::Regex::new(URL_PATTERN).expect("URL_PATTERN compiles");
    static FILE_PATH_RE: regex::Regex =
        regex::Regex::new(FILE_PATH_PATTERN).expect("FILE_PATH_PATTERN compiles");
}

/// What a detected link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// An auto-detected URL (the trailing-punctuation-trimmed match text).
    Url(String),
    /// An auto-detected `path:line[:col]` reference. `line` / `col` are
    /// 1-based; `col` defaults to 1 when the match omitted it.
    FilePath { path: String, line: u32, col: u32 },
}

/// A detected link plus the physical cell range(s) it occupies. Each
/// `(row, col_start, col_end)` is an inclusive-exclusive column span on
/// one physical viewport row (`col_start <= col < col_end`). A link that
/// crosses a soft-wrap boundary yields one entry per physical row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedLink {
    pub kind: LinkKind,
    pub cells: Vec<(u16, u16, u16)>,
}

/// One character of the logical line text, paired with the physical cell
/// it came from. Built cell-by-cell so a regex match's char range can be
/// translated back into physical `(row, col)` spans even across wide
/// glyphs and soft wraps.
struct CharCell {
    row: u16,
    col: u16,
    /// Physical width in cells (1 for normal, 2 for wide glyphs). Used so
    /// a matched span's `col_end` covers the glyph's full footprint.
    width: u16,
}

/// A logical line: the soft-wrap-joined text plus the per-char physical
/// mapping. `cells[i]` describes the physical origin of the `i`-th *char*
/// in `text` (in `char` units, matching how the JS regex indexes into the
/// concatenated string).
struct LogicalLine {
    text: String,
    cells: Vec<CharCell>,
}

/// Build a logical line by joining soft-wrapped physical lines that
/// contain `row`. Mirrors `getLogicalLine` in `url-detector.ts`:
/// `get_line_wrapped(r) == true` means "row `r` is a continuation of the
/// previous row", so we walk backward while the *current* row is wrapped
/// and forward while the *next* row is wrapped.
///
/// Wide glyphs (`get_cell_width == 2`) contribute their grapheme once and
/// occupy two physical columns; the continuation cell (`width == 0`) is
/// skipped so it does not inject a phantom char into `text`. Empty cells
/// contribute a single space, matching the WebView's `getCell(c).char ||
/// " "`.
fn build_logical_line(core: &TerminalCore, row: u16) -> LogicalLine {
    let rows = core.rows();
    let cols = core.cols();
    if rows == 0 || cols == 0 || row >= rows {
        return LogicalLine {
            text: String::new(),
            cells: Vec::new(),
        };
    }

    // Walk backward to the start of the logical line.
    let mut start_row = row;
    while start_row > 0 && core.get_line_wrapped(start_row) {
        start_row -= 1;
    }

    let mut text = String::new();
    let mut cells: Vec<CharCell> = Vec::new();
    let mut r = start_row;
    loop {
        for c in 0..cols {
            // Skip the trailing half of a wide glyph: it carries no char
            // of its own (width 0) and printing a space for it would
            // misalign the char→cell mapping.
            if core.get_cell_width(c, r) == 0 {
                continue;
            }
            let ch = core.get_cell_char(c, r);
            let ch = if ch.is_empty() { " ".to_string() } else { ch };
            let width = core.get_cell_width(c, r).max(1) as u16;
            // The JS side indexes the regex match in UTF-16 code units,
            // but since both URL and path matches are ASCII the per-char
            // (Rust `char`) granularity is equivalent for our spans. A
            // multi-codepoint grapheme (emoji ZWJ) can never be part of a
            // URL/path match, so collapsing it to one logical char is
            // safe and keeps the mapping aligned with the physical cell.
            //
            // One CharCell per `char` of `ch` keeps `cells[i]` aligned
            // with the i-th char of `text` (multi-char graphemes share a
            // physical cell). ASCII matches make this 1:1 in practice.
            for _ in ch.chars() {
                cells.push(CharCell {
                    row: r,
                    col: c,
                    width,
                });
            }
            text.push_str(&ch);
        }
        // Advance only while the *next* row is a continuation.
        if r + 1 < rows && core.get_line_wrapped(r + 1) {
            r += 1;
        } else {
            break;
        }
    }

    LogicalLine { text, cells }
}

/// Return the soft-wrap-joined text of the logical line that contains
/// `row`. This is the same string the link detectors regex-scan; callers
/// (e.g. `window_host.rs`) can cache it and skip re-detection when the
/// text is unchanged between PTY updates.
///
/// Text-only sibling of `build_logical_line`: it runs on every PTY pump
/// while the pointer rests over the grid, so it must not pay for the
/// per-char `Vec<CharCell>` mapping that detection needs but a text
/// comparison does not. Keep the row walk in sync with
/// `build_logical_line`.
pub fn logical_line_text(core: &TerminalCore, row: u16) -> String {
    let rows = core.rows();
    let cols = core.cols();
    if rows == 0 || cols == 0 || row >= rows {
        return String::new();
    }

    // Walk backward to the start of the logical line.
    let mut start_row = row;
    while start_row > 0 && core.get_line_wrapped(start_row) {
        start_row -= 1;
    }

    let mut text = String::new();
    let mut r = start_row;
    loop {
        for c in 0..cols {
            // Skip the trailing half of a wide glyph (width 0), same as
            // build_logical_line, so both producers emit identical text.
            if core.get_cell_width(c, r) == 0 {
                continue;
            }
            let ch = core.get_cell_char(c, r);
            if ch.is_empty() {
                text.push(' ');
            } else {
                text.push_str(&ch);
            }
        }
        // Advance only while the *next* row is a continuation.
        if r + 1 < rows && core.get_line_wrapped(r + 1) {
            r += 1;
        } else {
            break;
        }
    }
    text
}

/// Trim trailing punctuation that is likely not part of the match.
/// Mirrors the `/[.,;:!?)}\]>]$/` trim loops in `url-detector.ts`
/// (lines 125-127 and 211-213). Returns the trimmed `&str`.
fn trim_trailing_punct(s: &str) -> &str {
    let mut end = s.len();
    while end > 0 {
        let last = s.as_bytes()[end - 1];
        if matches!(
            last,
            b'.' | b',' | b';' | b':' | b'!' | b'?' | b')' | b'}' | b']' | b'>'
        ) {
            end -= 1;
        } else {
            break;
        }
    }
    &s[..end]
}

/// Number of `char`s in a `&str` (the unit the per-char `cells` map and
/// the regex byte→char conversion use).
fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Collapse a char range `[start_char, end_char)` of a logical line into
/// physical cell spans: one `(row, col_start, col_end)` per physical row
/// the range touches. Adjacent same-row cells are merged into a single
/// span; wide glyphs extend `col_end` by their full width.
fn char_range_to_cells(
    line: &LogicalLine,
    start_char: usize,
    end_char: usize,
) -> Vec<(u16, u16, u16)> {
    let mut spans: Vec<(u16, u16, u16)> = Vec::new();
    for cc in line
        .cells
        .iter()
        .skip(start_char)
        .take(end_char.saturating_sub(start_char))
    {
        let col_start = cc.col;
        // Extend by the glyph's physical width so a wide glyph covers both
        // of its cells. URL/path matches are ASCII (width 1) in practice;
        // this keeps the span correct regardless.
        let col_end = cc.col.saturating_add(cc.width.max(1));
        match spans.last_mut() {
            // Merge contiguous cells, and also coalesce duplicate /
            // already-covered cells (a multi-char grapheme emits several
            // CharCells sharing one physical column) so the span stays a
            // single contiguous run per row.
            Some((row, _cs, ce)) if *row == cc.row && col_start <= *ce => {
                *ce = (*ce).max(col_end);
            }
            _ => spans.push((cc.row, col_start, col_end)),
        }
    }
    spans
}

/// Find the URL or file path covering the physical cell `(row, col)` in
/// the (live) viewport. `detect_urls` / `detect_paths` gate the two
/// detectors independently. URLs take priority over file paths, matching
/// the WebView click/hover order in `link.ts`.
///
/// Returns `None` when nothing covers the cell or both detectors are off.
pub fn find_link_at(
    core: &TerminalCore,
    row: u16,
    col: u16,
    detect_urls: bool,
    detect_paths: bool,
) -> Option<DetectedLink> {
    if !detect_urls && !detect_paths {
        return None;
    }
    let line = build_logical_line(core, row);
    if line.text.is_empty() {
        return None;
    }
    // Logical char index of the hovered cell.
    let hover_char = logical_char_index(&line, row, col)?;

    if detect_urls {
        if let Some(link) = find_url_covering(&line, hover_char) {
            return Some(link);
        }
    }
    if detect_paths {
        if let Some(link) = find_file_path_covering(&line, hover_char) {
            return Some(link);
        }
    }
    None
}

/// Map a physical `(row, col)` to its logical char index within `line`,
/// or `None` if the cell is not part of this logical line (e.g. a
/// width-0 continuation cell that was skipped during build).
fn logical_char_index(line: &LogicalLine, row: u16, col: u16) -> Option<usize> {
    line.cells
        .iter()
        .position(|cc| cc.row == row && cc.col == col)
}

/// Scan URL matches and return the one covering `hover_char` (in char
/// units), trimmed and cell-mapped. Mirrors `detectUrls` +
/// `findUrlAtPosition`.
fn find_url_covering(line: &LogicalLine, hover_char: usize) -> Option<DetectedLink> {
    URL_RE.with(|re| {
        // Incremental byte→char cursor: find_iter yields matches in ascending
        // byte order, so we can walk forward from the previous match position
        // rather than counting from the start of the string each time.
        // This reduces the total work from O(matches * line_length) to
        // O(line_length).
        let mut last_byte: usize = 0;
        let mut last_char: usize = 0;

        for m in re.find_iter(&line.text) {
            let raw = m.as_str();
            let trimmed = trim_trailing_punct(raw);
            if trimmed.is_empty() {
                continue;
            }
            // Advance the char cursor from `last_byte` to `m.start()`.
            debug_assert!(
                m.start() >= last_byte,
                "find_iter must yield matches in ascending byte order"
            );
            last_char += line.text[last_byte..m.start()].chars().count();
            last_byte = m.start();

            let start_char = last_char;
            let end_char = start_char + char_len(trimmed);

            // Advance cursor to the end of this match so the next iteration
            // starts from here (not from the start of the string).
            last_char += line.text[last_byte..m.end()].chars().count();
            last_byte = m.end();

            if hover_char >= start_char && hover_char < end_char {
                return Some(DetectedLink {
                    kind: LinkKind::Url(trimmed.to_string()),
                    cells: char_range_to_cells(line, start_char, end_char),
                });
            }
        }
        None
    })
}

/// Scan file-path matches and return the one covering `hover_char`,
/// excluding matches that are actually the tail of a URL. Mirrors
/// `detectFilePaths` + `findFilePathAtPosition`.
fn find_file_path_covering(line: &LogicalLine, hover_char: usize) -> Option<DetectedLink> {
    let text = &line.text;

    // Collect all URL byte ranges in one pass so that the per-file-path-match
    // "is this a URL tail?" check can be done in O(U+P) using a cursor rather
    // than O(U*P) with `.any(...)`. URL ranges from `find_iter` are in
    // ascending start-byte order.
    let url_ranges: Vec<(usize, usize)> =
        URL_RE.with(|re| re.find_iter(text).map(|m| (m.start(), m.end())).collect());

    FILE_PATH_RE.with(|re| {
        // Cursor into `url_ranges`: all entries before this index have
        // `ue < start_byte` for every file-path match seen so far, so they
        // can never satisfy the overlap condition and are skipped permanently.
        let mut url_cursor = 0usize;

        // Incremental byte→char cursor: find_iter yields matches in ascending
        // byte order, so we advance forward from the previous position instead
        // of counting from the string start each time.
        let mut last_byte: usize = 0;
        let mut last_char: usize = 0;

        for m in re.find_iter(text) {
            let start_byte = m.start();
            let raw_full = m.as_str();

            // Advance the cursor past URL ranges that end before `start_byte`;
            // they cannot contain `start_byte` regardless of where they start.
            while url_cursor < url_ranges.len() && url_ranges[url_cursor].1 < start_byte {
                url_cursor += 1;
            }

            // Exclude matches embedded in a URL: the overlap condition is
            // `us < start_byte && ue >= start_byte`. After the cursor advance
            // every remaining candidate has `ue >= start_byte`; we only need
            // to confirm `us < start_byte` for the first such entry, because
            // URL ranges are sorted by `us` — if the first entry has
            // `us >= start_byte`, none of the later ones can have
            // `us < start_byte` either, so we can stop checking immediately.
            // Mirrors the `URL_PROTOCOL_PREFIX` lookbehind in `detectFilePaths`.
            let covered_by_url =
                url_cursor < url_ranges.len() && url_ranges[url_cursor].0 < start_byte;
            if covered_by_url {
                // Still advance the byte→char cursor to keep it consistent
                // for subsequent non-skipped matches.
                debug_assert!(
                    start_byte >= last_byte,
                    "find_iter must yield matches in ascending byte order"
                );
                last_char += text[last_byte..start_byte].chars().count();
                last_char += text[start_byte..m.end()].chars().count();
                last_byte = m.end();
                continue;
            }

            // Trim trailing punctuation (error messages like "foo.ts:42.").
            let raw = trim_trailing_punct(raw_full);

            // Must still contain ':' (the line-number separator).
            let colon_idx = match raw.find(':') {
                Some(i) => i,
                None => {
                    // Advance cursor even when we skip.
                    debug_assert!(start_byte >= last_byte);
                    last_char += text[last_byte..m.end()].chars().count();
                    last_byte = m.end();
                    continue;
                }
            };
            let path = &raw[..colon_idx];
            let rest = &raw[colon_idx + 1..];

            // Reject bare filenames / time patterns: require a path
            // component (contains '/' or starts with ./ ../ /).
            if !path.contains('/')
                && !path.starts_with("./")
                && !path.starts_with("../")
                && !path.starts_with('/')
            {
                debug_assert!(start_byte >= last_byte);
                last_char += text[last_byte..m.end()].chars().count();
                last_byte = m.end();
                continue;
            }

            let mut parts = rest.split(':');
            let line_no: u32 = match parts.next().and_then(|p| p.parse::<u32>().ok()) {
                Some(n) if n > 0 => n,
                _ => {
                    debug_assert!(start_byte >= last_byte);
                    last_char += text[last_byte..m.end()].chars().count();
                    last_byte = m.end();
                    continue;
                }
            };
            let col_no: u32 = match parts.next().and_then(|p| p.parse::<u32>().ok()) {
                Some(n) if n > 0 => n,
                _ => 1,
            };

            // Advance the incremental cursor from `last_byte` to `start_byte`.
            debug_assert!(
                start_byte >= last_byte,
                "find_iter must yield matches in ascending byte order"
            );
            last_char += text[last_byte..start_byte].chars().count();
            last_byte = start_byte;

            let start_char = last_char;
            let end_char = start_char + char_len(raw);

            // Advance cursor to end of this match.
            last_char += text[last_byte..m.end()].chars().count();
            last_byte = m.end();

            if hover_char >= start_char && hover_char < end_char {
                return Some(DetectedLink {
                    kind: LinkKind::FilePath {
                        path: path.to_string(),
                        line: line_no,
                        col: col_no,
                    },
                    cells: char_range_to_cells(line, start_char, end_char),
                });
            }
        }
        None
    })
}

/// Resolve a clicked file path against the shell CWD (from OSC 7) and
/// return the absolute path. Absolute paths (`/...`) pass through.
/// Mirrors the resolution half of `openFileInEditor` in `link.ts`
/// (lines 127-145): a `file://[host]/path` CWD has its scheme + host
/// stripped and is percent-decoded; a bare path is used verbatim.
///
/// A relative path with no usable CWD is returned as-is, per the
/// file-path-click SPEC FR6 ("If CWD is empty, pass relative path
/// as-is to editor") — the caller's click-time existence check then
/// decides whether anything actually opens.
pub fn resolve_path(file_path: &str, cwd: Option<&str>) -> String {
    if file_path.starts_with('/') {
        return file_path.to_string();
    }
    match cwd {
        Some(c) if !c.is_empty() => {
            let clean = clean_cwd(c);
            format!("{clean}/{file_path}")
        }
        _ => file_path.to_string(),
    }
}

/// Strip a `file://[host]` prefix and percent-decode the path portion.
/// `file:///path` → `/path`; `file://host/path` → `/path`. A bare path
/// is returned percent-decoded only if it carried a `file://` scheme
/// (otherwise verbatim, matching `link.ts`'s `else` branch).
fn clean_cwd(cwd: &str) -> String {
    if let Some(rest) = cwd.strip_prefix("file://") {
        // Drop the optional host segment before the first '/'.
        let path = match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => rest,
        };
        percent_decode(path)
    } else {
        cwd.to_string()
    }
}

/// Minimal `%XX` decoder for ASCII bytes; multi-byte UTF-8 sequences are
/// reassembled when the decoded bytes form valid UTF-8. Malformed escapes
/// pass through verbatim. Matches the WebView's `decodeURIComponent`
/// closely enough for filesystem paths.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).into_owned())
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Whether `uri` has a safe scheme to hand to the OS opener. Only
/// `http`/`https`/`mailto`/`ssh` are allowed; everything else (including
/// `file:`, `javascript:`, `data:`) is blocked. Mirrors `isSafeUri` in
/// `link.ts` (lines 294-303).
pub fn is_safe_uri(uri: &str) -> bool {
    const SAFE_SCHEMES: [&str; 4] = ["http", "https", "mailto", "ssh"];
    // Scheme = chars up to the first ':' (ASCII letter / digit / +-.).
    match uri.find(':') {
        Some(idx) => {
            let scheme = &uri[..idx];
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                && SAFE_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str())
        }
        None => false,
    }
}

/// Expand one whitespace-delimited token from an editor-command template by
/// substituting `{file}`, `{line}`, and `{col}` in a single left-to-right
/// pass. The input text is never re-scanned after a substitution, so values
/// that happen to contain placeholder-like strings (e.g. a file path that
/// literally includes `{line}`) are emitted verbatim without a second
/// replacement.
fn expand_token(token: &str, file: &str, line_s: &str, col_s: &str) -> String {
    let mut out = String::with_capacity(token.len() + file.len());
    let mut rest = token;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("{file}") {
            out.push_str(file);
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix("{line}") {
            out.push_str(line_s);
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix("{col}") {
            out.push_str(col_s);
            rest = stripped;
        } else {
            // Advance one byte at a time; all placeholders are ASCII so a
            // non-placeholder '{' is safe to copy character-by-character.
            let next_char_len = rest.chars().next().map_or(1, |c| c.len_utf8());
            out.push_str(&rest[..next_char_len]);
            rest = &rest[next_char_len..];
        }
    }
    out
}

/// Expand an editor-command template into `(program, args)`. The template
/// is split on whitespace *before* placeholder expansion so spaces inside
/// a resolved file path do not break argument boundaries. `{file}`,
/// `{line}`, `{col}` are substituted in every non-program token. Mirrors
/// the token logic in `openFileInEditor` (`link.ts` lines 163-176).
///
/// Returns `None` when the template is blank.
pub fn build_editor_command(
    template: &str,
    file: &str,
    line: u32,
    col: u32,
) -> Option<(String, Vec<String>)> {
    let mut tokens = template.split_whitespace();
    let program = tokens.next()?.to_string();
    let line_s = line.to_string();
    let col_s = col.to_string();
    let args: Vec<String> = tokens
        .map(|t| expand_token(t, file, &line_s, &col_s))
        .collect();
    Some((program, args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_core::terminal_core::TerminalCore;

    fn core_with(cols: u16, rows: u16, input: &[u8]) -> TerminalCore {
        let mut core = TerminalCore::new(cols, rows, 100);
        core.process_pty_data(input);
        core
    }

    /// Cell at `(row, col)` whose char equals `expect` — sanity helper for
    /// the wide-char mapping tests.
    fn assert_cell(core: &TerminalCore, row: u16, col: u16, expect: &str) {
        assert_eq!(core.get_cell_char(col, row), expect, "cell ({row},{col})");
    }

    // ── URL detection ────────────────────────────────────────

    #[test]
    fn detects_https_url() {
        let core = core_with(80, 3, b"see https://example.com/path now");
        let link = find_link_at(&core, 0, 10, true, true).expect("link");
        assert_eq!(
            link.kind,
            LinkKind::Url("https://example.com/path".to_string())
        );
    }

    #[test]
    fn detects_http_ftp_file_protocols() {
        for (proto, body) in [
            ("http", "http://a.b/c"),
            ("ftp", "ftp://h/f"),
            ("file", "file:///etc/hosts"),
        ] {
            let core = core_with(80, 3, body.as_bytes());
            let link = find_link_at(&core, 0, 2, true, true)
                .unwrap_or_else(|| panic!("{proto} should detect"));
            assert_eq!(link.kind, LinkKind::Url(body.to_string()));
        }
    }

    #[test]
    fn trims_trailing_punctuation() {
        // Trailing ").," should be trimmed off the URL.
        let core = core_with(80, 3, b"(https://example.com/path).");
        let link = find_link_at(&core, 0, 5, true, true).expect("link");
        assert_eq!(
            link.kind,
            LinkKind::Url("https://example.com/path".to_string())
        );
    }

    #[test]
    fn url_in_parentheses_excludes_close_paren() {
        // The regex stops at ')' so the closing paren is never part of the
        // match in the first place.
        let core = core_with(80, 3, b"x (https://e.com/p) y");
        let link = find_link_at(&core, 0, 10, true, true).expect("link");
        assert_eq!(link.kind, LinkKind::Url("https://e.com/p".to_string()));
    }

    #[test]
    fn no_url_when_disabled() {
        let core = core_with(80, 3, b"https://example.com/path");
        assert!(find_link_at(&core, 0, 5, false, true).is_none());
    }

    #[test]
    fn no_link_off_the_match() {
        let core = core_with(80, 3, b"see https://example.com end");
        // Column 0 ('s' of "see") is outside the URL span.
        assert!(find_link_at(&core, 0, 0, true, true).is_none());
    }

    // ── File-path detection ──────────────────────────────────

    #[test]
    fn detects_relative_path_with_line() {
        let core = core_with(80, 3, b"at src/foo.ts:42 here");
        let link = find_link_at(&core, 0, 5, true, true).expect("link");
        assert_eq!(
            link.kind,
            LinkKind::FilePath {
                path: "src/foo.ts".to_string(),
                line: 42,
                col: 1,
            }
        );
    }

    #[test]
    fn detects_path_with_line_and_col() {
        let core = core_with(80, 3, b"../lib/bar.py:5:3");
        let link = find_link_at(&core, 0, 2, true, true).expect("link");
        assert_eq!(
            link.kind,
            LinkKind::FilePath {
                path: "../lib/bar.py".to_string(),
                line: 5,
                col: 3,
            }
        );
    }

    #[test]
    fn detects_absolute_path() {
        let core = core_with(80, 3, b"/home/user/file.rs:10");
        let link = find_link_at(&core, 0, 3, true, true).expect("link");
        assert_eq!(
            link.kind,
            LinkKind::FilePath {
                path: "/home/user/file.rs".to_string(),
                line: 10,
                col: 1,
            }
        );
    }

    #[test]
    fn detects_dot_slash_path() {
        let core = core_with(80, 3, b"./src/foo.ts:42");
        let link = find_link_at(&core, 0, 1, true, true).expect("link");
        assert_eq!(
            link.kind,
            LinkKind::FilePath {
                path: "./src/foo.ts".to_string(),
                line: 42,
                col: 1,
            }
        );
    }

    #[test]
    fn rejects_bare_filename_without_path() {
        // "foo.ts:42" has no '/' and no ./ ../ / prefix → not a path.
        let core = core_with(80, 3, b"foo.ts:42");
        assert!(find_link_at(&core, 0, 1, false, true).is_none());
    }

    #[test]
    fn url_not_detected_as_file_path() {
        // With only path detection on, the host:port tail of a URL must
        // not be picked up as a file path.
        let core = core_with(80, 3, b"http://example.com/a.txt:80");
        assert!(find_link_at(&core, 0, 20, false, true).is_none());
    }

    #[test]
    fn url_priority_over_file_path() {
        // Both enabled: a URL covering the cell wins over any path match.
        let core = core_with(80, 3, b"http://example.com/a.txt:80");
        let link = find_link_at(&core, 0, 20, true, true).expect("link");
        match link.kind {
            LinkKind::Url(_) => {}
            other => panic!("expected URL, got {other:?}"),
        }
    }

    // ── Multiple links on one line (incremental char-index correctness) ─

    /// Two URLs on the same logical line: hover over the *second* one must
    /// yield the correct trimmed URL and correct cell span. This validates
    /// that the incremental byte→char cursor does not mis-count the char
    /// offset of the second match.
    #[test]
    fn two_urls_on_same_line_second_url_detected_correctly() {
        // "see http://a.io/ and http://b.io/x now"
        // The second URL starts at byte offset 21 (after "see http://a.io/ and ").
        let input = b"see http://a.io/ and http://b.io/x now";
        let core = core_with(80, 3, input);
        // Hover on a char that falls inside the second URL ("http://b.io/x").
        // "see http://a.io/ and " is 21 bytes/chars; the second URL starts at
        // col 21.  Hover at col 25 (inside "b.io").
        let link = find_link_at(&core, 0, 25, true, true).expect("second URL");
        assert_eq!(
            link.kind,
            LinkKind::Url("http://b.io/x".to_string()),
            "second URL kind"
        );
        // The cell span must start at col 21, not col 0 or some earlier URL.
        let first_col = link.cells.iter().map(|(_, cs, _)| *cs).min().unwrap();
        assert_eq!(
            first_col, 21,
            "second URL starts at col 21: {:?}",
            link.cells
        );
    }

    /// Two file paths on the same logical line: hover over the *second* one
    /// must yield the correct path and correct cell span.
    #[test]
    fn two_file_paths_on_same_line_second_path_detected_correctly() {
        // "src/a.ts:1 and src/b.ts:2"
        let input = b"src/a.ts:1 and src/b.ts:2";
        let core = core_with(80, 3, input);
        // "src/a.ts:1 and " is 15 chars; second path starts at col 15.
        // Hover at col 18 (inside "src/b.ts").
        let link = find_link_at(&core, 0, 18, false, true).expect("second path");
        assert_eq!(
            link.kind,
            LinkKind::FilePath {
                path: "src/b.ts".to_string(),
                line: 2,
                col: 1,
            },
            "second path kind"
        );
        let first_col = link.cells.iter().map(|(_, cs, _)| *cs).min().unwrap();
        assert_eq!(
            first_col, 15,
            "second path starts at col 15: {:?}",
            link.cells
        );
    }

    // ── Soft-wrap across rows ────────────────────────────────

    #[test]
    fn url_spanning_wrapped_rows() {
        // 10-col grid; a URL longer than 10 chars wraps to the next row.
        // term_core sets ring_wrapped[row1] = true on the continuation.
        let core = core_with(10, 3, b"https://example.com/p");
        // Confirm it actually wrapped.
        assert!(core.get_line_wrapped(1), "row 1 should be a continuation");
        // Hover on row 0 (within the URL head).
        let link = find_link_at(&core, 0, 5, true, true).expect("link on row0");
        assert_eq!(
            link.kind,
            LinkKind::Url("https://example.com/p".to_string())
        );
        // The cell spans should cover both physical rows.
        let rows: std::collections::HashSet<u16> = link.cells.iter().map(|(r, _, _)| *r).collect();
        assert!(
            rows.contains(&0) && rows.contains(&1),
            "spans both rows: {:?}",
            link.cells
        );
        // Hover on row 1 (the wrapped tail) finds the same URL.
        let link2 = find_link_at(&core, 1, 0, true, true).expect("link on row1");
        assert_eq!(link2.kind, link.kind);
    }

    // ── Wide-char cell mapping ───────────────────────────────

    #[test]
    fn wide_char_cell_mapping() {
        // A double-width CJK glyph before a URL shifts the URL's physical
        // columns by 2 even though it is one logical char. "あ" is width 2.
        let core = core_with(80, 3, "あ http://e.com/p".as_bytes());
        // Sanity: the wide glyph occupies col 0 (width 2), col 1 is the
        // continuation (width 0), the space is col 2, 'h' starts at col 3.
        assert_cell(&core, 0, 0, "あ");
        assert_eq!(core.get_cell_width(0, 0), 2);
        assert_eq!(core.get_cell_width(1, 0), 0);
        assert_cell(&core, 0, 3, "h");
        let link = find_link_at(&core, 0, 6, true, true).expect("link");
        assert_eq!(link.kind, LinkKind::Url("http://e.com/p".to_string()));
        // First cell of the URL span must be physical col 3 (after the
        // wide glyph + space), not col 1.
        let first_col = link.cells.iter().map(|(_, cs, _)| *cs).min().unwrap();
        assert_eq!(
            first_col, 3,
            "URL starts after the wide glyph: {:?}",
            link.cells
        );
    }

    // ── CWD resolution ───────────────────────────────────────

    #[test]
    fn resolve_absolute_path_passthrough() {
        assert_eq!(
            resolve_path("/etc/hosts", Some("file:///home/u")),
            "/etc/hosts".to_string()
        );
    }

    #[test]
    fn resolve_relative_with_file_uri_cwd() {
        assert_eq!(
            resolve_path("src/a.rs", Some("file:///home/u/proj")),
            "/home/u/proj/src/a.rs".to_string()
        );
    }

    #[test]
    fn resolve_relative_with_host_and_percent_encoding() {
        // file://host/path with %20 → host stripped, %20 decoded.
        assert_eq!(
            resolve_path("a.rs", Some("file://myhost/home/my%20proj")),
            "/home/my proj/a.rs".to_string()
        );
    }

    #[test]
    fn resolve_relative_with_plain_cwd() {
        assert_eq!(
            resolve_path("a.rs", Some("/home/u")),
            "/home/u/a.rs".to_string()
        );
    }

    #[test]
    fn resolve_relative_without_cwd_passes_through_as_is() {
        // SPEC FR6: "If CWD is empty, pass relative path as-is to editor".
        assert_eq!(resolve_path("a.rs", None), "a.rs".to_string());
        assert_eq!(resolve_path("a.rs", Some("")), "a.rs".to_string());
    }

    // ── Safe-URI whitelist ───────────────────────────────────

    #[test]
    fn safe_uri_allows_whitelisted_schemes() {
        assert!(is_safe_uri("http://e.com"));
        assert!(is_safe_uri("https://e.com"));
        assert!(is_safe_uri("mailto:a@b.com"));
        assert!(is_safe_uri("ssh://host"));
        assert!(is_safe_uri("HTTPS://e.com")); // case-insensitive scheme
    }

    #[test]
    fn safe_uri_blocks_dangerous_schemes() {
        assert!(!is_safe_uri("file:///etc/passwd"));
        assert!(!is_safe_uri("javascript:alert(1)"));
        assert!(!is_safe_uri("data:text/html,x"));
        assert!(!is_safe_uri("ftp://h/f"));
        assert!(!is_safe_uri("relative/path"));
        assert!(!is_safe_uri(""));
    }

    // ── Editor-command templating ────────────────────────────

    #[test]
    fn editor_command_expands_placeholders() {
        let (prog, args) =
            build_editor_command("code --goto {file}:{line}:{col}", "/x/y.rs", 12, 3).expect("cmd");
        assert_eq!(prog, "code");
        assert_eq!(args, vec!["--goto", "/x/y.rs:12:3"]);
    }

    #[test]
    fn editor_command_path_with_spaces_keeps_one_arg() {
        // Spaces inside the resolved path do not split the token because
        // the template is tokenized before expansion.
        let (prog, args) = build_editor_command("nvim {file}", "/my proj/a.rs", 1, 1).expect("cmd");
        assert_eq!(prog, "nvim");
        assert_eq!(args, vec!["/my proj/a.rs"]);
    }

    #[test]
    fn editor_command_blank_is_none() {
        assert!(build_editor_command("   ", "/x", 1, 1).is_none());
        assert!(build_editor_command("", "/x", 1, 1).is_none());
    }

    /// A file path that literally contains `{line}` or `{col}` must not be
    /// corrupted by a second substitution pass. On Linux, brace characters
    /// are legal in file names, so this is a realistic edge case when the
    /// path originates from untrusted PTY output.
    #[test]
    fn editor_command_path_with_placeholder_literals_not_double_substituted() {
        // The resolved path already contains "{line}" as part of the name.
        // The one-pass expander must emit it verbatim, not replace it again.
        let path = "/proj/{line}/src/a.rs";
        let (prog, args) =
            build_editor_command("code --goto {file}:{line}:{col}", path, 5, 2).expect("cmd");
        assert_eq!(prog, "code");
        // `{file}` → "/proj/{line}/src/a.rs", `{line}` → "5", `{col}` → "2"
        // The `{line}` inside the already-substituted file value must survive.
        assert_eq!(args, vec!["--goto", "/proj/{line}/src/a.rs:5:2"]);
    }
}
