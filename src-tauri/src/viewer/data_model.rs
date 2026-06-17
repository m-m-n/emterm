//! JSON/YAML data-viewer model — the window-free, fully unit-testable
//! half of the viewer (Rust port of the WebView `src/data-viewer/`
//! parser / tree-builder / highlighter / fullscreen-controller logic).
//!
//! Owns: parsing (serde_json / serde_yml), the always-fully-expanded
//! outline tree, path resolution + detail re-serialization in the source
//! format, the token highlighter (key / string / number / boolean / null
//! / punctuation / comment), the outline-vs-RAW mode state, the JSON
//! pretty-print toggle, and the keyboard-navigation arithmetic.

use super::data::DataFormat;

// ── Parsing ────────────────────────────────────────────────────────────

/// Parsed document, kept in its source-native value type so the detail
/// pane can re-serialize subtrees in the ORIGINAL format (FR4).
#[derive(Debug)]
pub enum Parsed {
    Json(serde_json::Value),
    Yaml(serde_yml::Value),
}

/// Parse `text` as `format`. `Err` carries the parser's message for the
/// error banner (FR9).
pub fn parse(format: DataFormat, text: &str) -> Result<Parsed, String> {
    match format {
        DataFormat::Json => serde_json::from_str(text)
            .map(Parsed::Json)
            .map_err(|e| e.to_string()),
        DataFormat::Yaml => serde_yml::from_str(text)
            .map(Parsed::Yaml)
            .map_err(|e| e.to_string()),
    }
}

// ── Outline tree ───────────────────────────────────────────────────────

/// One step of a path from the document root to a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    Key(String),
    Index(usize),
}

/// One row of the always-fully-expanded outline tree (WebView
/// `tree-builder.ts`). `nodes[0]` is always the `(root)` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    /// 0 for root, +1 per nesting level.
    pub depth: usize,
    /// Display label: object key as-is, array element as `[i]`,
    /// `(root)` for the root row.
    pub label: String,
    /// True when the node is a non-empty object/array (`▸` marker).
    pub has_children: bool,
    /// Path from the root to this node (empty for root).
    pub path: Vec<PathSeg>,
}

/// Flatten the document into tree rows in document order, root first.
pub fn build_tree(parsed: &Parsed) -> Vec<TreeNode> {
    let mut nodes = Vec::new();
    nodes.push(TreeNode {
        depth: 0,
        label: "(root)".to_string(),
        has_children: match parsed {
            Parsed::Json(v) => json_has_children(v),
            Parsed::Yaml(v) => yaml_has_children(v),
        },
        path: Vec::new(),
    });
    match parsed {
        Parsed::Json(v) => walk_json(v, 1, &mut Vec::new(), &mut nodes),
        Parsed::Yaml(v) => walk_yaml(v, 1, &mut Vec::new(), &mut nodes),
    }
    nodes
}

fn json_has_children(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(m) => !m.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        _ => false,
    }
}

fn walk_json(
    v: &serde_json::Value,
    depth: usize,
    path: &mut Vec<PathSeg>,
    out: &mut Vec<TreeNode>,
) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, child) in m {
                path.push(PathSeg::Key(k.clone()));
                out.push(TreeNode {
                    depth,
                    label: k.clone(),
                    has_children: json_has_children(child),
                    path: path.clone(),
                });
                walk_json(child, depth + 1, path, out);
                path.pop();
            }
        }
        serde_json::Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                path.push(PathSeg::Index(i));
                out.push(TreeNode {
                    depth,
                    label: format!("[{i}]"),
                    has_children: json_has_children(child),
                    path: path.clone(),
                });
                walk_json(child, depth + 1, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

fn yaml_has_children(v: &serde_yml::Value) -> bool {
    match v {
        serde_yml::Value::Mapping(m) => !m.is_empty(),
        serde_yml::Value::Sequence(s) => !s.is_empty(),
        serde_yml::Value::Tagged(t) => yaml_has_children(&t.value),
        _ => false,
    }
}

/// Render a YAML mapping key as a tree label. Non-string keys (numbers,
/// booleans) are stringified.
fn yaml_key_label(k: &serde_yml::Value) -> String {
    match k {
        serde_yml::Value::String(s) => s.clone(),
        other => serde_yml::to_string(other)
            .map(|s| s.trim_end().to_string())
            .unwrap_or_else(|_| "?".to_string()),
    }
}

fn walk_yaml(v: &serde_yml::Value, depth: usize, path: &mut Vec<PathSeg>, out: &mut Vec<TreeNode>) {
    match v {
        serde_yml::Value::Mapping(m) => {
            for (k, child) in m {
                let label = yaml_key_label(k);
                path.push(PathSeg::Key(label.clone()));
                out.push(TreeNode {
                    depth,
                    label,
                    has_children: yaml_has_children(child),
                    path: path.clone(),
                });
                walk_yaml(child, depth + 1, path, out);
                path.pop();
            }
        }
        serde_yml::Value::Sequence(s) => {
            for (i, child) in s.iter().enumerate() {
                path.push(PathSeg::Index(i));
                out.push(TreeNode {
                    depth,
                    label: format!("[{i}]"),
                    has_children: yaml_has_children(child),
                    path: path.clone(),
                });
                walk_yaml(child, depth + 1, path, out);
                path.pop();
            }
        }
        // Anchors/aliases are resolved by the parser; tags wrap a value.
        serde_yml::Value::Tagged(t) => walk_yaml(&t.value, depth, path, out),
        _ => {}
    }
}

// ── Detail rendering ───────────────────────────────────────────────────

/// Serialize the node at `path` in the document's ORIGINAL format with
/// 2-space indent (WebView parser.ts: `JSON.stringify(data, null, 2)` /
/// `YAML.stringify(data, {indent: 2})`). Empty path = whole document.
pub fn detail_text(parsed: &Parsed, path: &[PathSeg]) -> String {
    match parsed {
        Parsed::Json(root) => {
            let mut cur = root;
            for seg in path {
                cur = match (seg, cur) {
                    (PathSeg::Key(k), serde_json::Value::Object(m)) => match m.get(k) {
                        Some(v) => v,
                        None => return String::new(),
                    },
                    (PathSeg::Index(i), serde_json::Value::Array(a)) => match a.get(*i) {
                        Some(v) => v,
                        None => return String::new(),
                    },
                    _ => return String::new(),
                };
            }
            serde_json::to_string_pretty(cur).unwrap_or_default()
        }
        Parsed::Yaml(root) => {
            let mut cur = root;
            for seg in path {
                // Tags are transparent for navigation (the tree walks
                // through them too).
                while let serde_yml::Value::Tagged(t) = cur {
                    cur = &t.value;
                }
                cur = match (seg, cur) {
                    (PathSeg::Key(k), serde_yml::Value::Mapping(m)) => {
                        match m.get(serde_yml::Value::String(k.clone())) {
                            Some(v) => v,
                            // Non-string YAML keys were stringified for the
                            // path; fall back to a label-match scan.
                            None => match m.iter().find(|(mk, _)| yaml_key_label(mk) == *k) {
                                Some((_, v)) => v,
                                None => return String::new(),
                            },
                        }
                    }
                    (PathSeg::Index(i), serde_yml::Value::Sequence(s)) => match s.get(*i) {
                        Some(v) => v,
                        None => return String::new(),
                    },
                    _ => return String::new(),
                };
            }
            serde_yml::to_string(cur).unwrap_or_default()
        }
    }
}

/// Pretty-print the JSON document (RAW-view `p` toggle, FR7). `None` for
/// YAML or when the document failed to parse.
pub fn pretty_json(parsed: &Parsed) -> Option<String> {
    match parsed {
        Parsed::Json(v) => serde_json::to_string_pretty(v).ok(),
        Parsed::Yaml(_) => None,
    }
}

// ── Syntax highlighting ────────────────────────────────────────────────

/// Token classes, mirroring the WebView `.dv-*` CSS classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    /// Object key (`.dv-key`, #9cdcfe).
    Key,
    /// String value (`.dv-string`, #ce9178).
    Str,
    /// Number (`.dv-number`, #b5cea8).
    Num,
    /// Boolean (`.dv-boolean`, #569cd6).
    Bool,
    /// Null (`.dv-null`, #569cd6 italic).
    Null,
    /// Punctuation (`.dv-punct`, #808080).
    Punct,
    /// YAML comment (`.dv-comment`, #6a9955 italic).
    Comment,
    /// Anything else (default foreground #d4d4d4).
    Plain,
}

/// Tokenize one line of JSON. Strings cannot contain raw newlines in
/// JSON, so per-line tokenization is exact for any document; a string
/// followed by `:` is a key.
pub fn highlight_json_line(line: &str) -> Vec<(TokKind, String)> {
    let bytes = line.as_bytes();
    let mut out: Vec<(TokKind, String)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => {
                // Scan the string (with escapes).
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                let end = i.min(bytes.len());
                // Key iff the next non-space char is ':'.
                let mut j = end;
                while j < bytes.len() && bytes[j] == b' ' {
                    j += 1;
                }
                let kind = if j < bytes.len() && bytes[j] == b':' {
                    TokKind::Key
                } else {
                    TokKind::Str
                };
                push_tok(&mut out, kind, &line[start..end]);
            }
            b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                push_tok(&mut out, TokKind::Punct, &line[i..i + 1]);
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    i += 1;
                }
                push_tok(&mut out, TokKind::Num, &line[start..i]);
            }
            b't' | b'f' if matches_at(line, i, "true") || matches_at(line, i, "false") => {
                let len = if matches_at(line, i, "true") { 4 } else { 5 };
                push_tok(&mut out, TokKind::Bool, &line[i..i + len]);
                i += len;
            }
            b'n' if matches_at(line, i, "null") => {
                push_tok(&mut out, TokKind::Null, &line[i..i + 4]);
                i += 4;
            }
            _ => {
                // Consume one UTF-8 scalar as plain text.
                let ch_len = utf8_len(b);
                let end = (i + ch_len).min(bytes.len());
                push_tok(&mut out, TokKind::Plain, &line[i..end]);
                i = end;
            }
        }
    }
    out
}

/// Tokenize one line of YAML (WebView highlighter.ts: line-based —
/// full-line comments, `key:` prefixes, `-` list markers, scalar
/// classification).
pub fn highlight_yaml_line(line: &str) -> Vec<(TokKind, String)> {
    let mut out: Vec<(TokKind, String)> = Vec::new();
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        push_tok(&mut out, TokKind::Comment, line);
        return out;
    }

    let indent_len = line.len() - trimmed.len();
    push_tok(&mut out, TokKind::Plain, &line[..indent_len]);

    // Leading list marker(s): `- ` (possibly `- - `).
    let mut rest = &line[indent_len..];
    while let Some(after) = rest.strip_prefix("- ") {
        push_tok(&mut out, TokKind::Punct, "- ");
        rest = after;
    }
    if rest == "-" {
        push_tok(&mut out, TokKind::Punct, "-");
        return out;
    }

    // `key:` prefix — find a ':' that ends the line or is followed by a
    // space (so URLs like http:// in values don't split).
    if let Some(colon) = find_yaml_key_colon(rest) {
        push_tok(&mut out, TokKind::Key, &rest[..colon]);
        push_tok(&mut out, TokKind::Punct, ":");
        let value = &rest[colon + 1..];
        if !value.is_empty() {
            highlight_yaml_scalar(value, &mut out);
        }
        return out;
    }

    // No key — a bare scalar (sequence entry value or continuation).
    highlight_yaml_scalar(rest, &mut out);
    out
}

/// Find the colon terminating a YAML key in `s` (a line with indent and
/// list markers already stripped): the first `:` at end-of-string or
/// followed by whitespace, not inside quotes.
fn find_yaml_key_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match in_quote {
            Some(q) => {
                if b == q {
                    in_quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => in_quote = Some(b),
                b'#' => return None, // comment starts; no key on this line
                b':' => {
                    if i + 1 == bytes.len() || bytes[i + 1] == b' ' || bytes[i + 1] == b'\t' {
                        return Some(i);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// Classify a YAML scalar (the value part after `key:` or a list `- `),
/// including a trailing ` # comment`.
fn highlight_yaml_scalar(s: &str, out: &mut Vec<(TokKind, String)>) {
    // Split off an unquoted trailing comment.
    let (value_part, comment_part) = split_yaml_comment(s);
    let v = value_part.trim();
    if v.is_empty() {
        push_tok(out, TokKind::Plain, value_part);
    } else {
        let lead_len = value_part.len() - value_part.trim_start().len();
        push_tok(out, TokKind::Plain, &value_part[..lead_len]);
        let trail_start = lead_len + v.len();
        let kind = classify_yaml_scalar(v);
        push_tok(out, kind, v);
        push_tok(out, TokKind::Plain, &value_part[trail_start..]);
    }
    if !comment_part.is_empty() {
        push_tok(out, TokKind::Comment, comment_part);
    }
}

fn classify_yaml_scalar(v: &str) -> TokKind {
    if v.starts_with('"') || v.starts_with('\'') {
        return TokKind::Str;
    }
    match v {
        "true" | "false" | "True" | "False" => return TokKind::Bool,
        "null" | "~" | "Null" | "NULL" => return TokKind::Null,
        _ => {}
    }
    if v.parse::<f64>().is_ok() {
        return TokKind::Num;
    }
    // Bare scalars are strings in YAML.
    TokKind::Str
}

/// Split `s` into (value, comment) at an unquoted ` #`.
fn split_yaml_comment(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    let mut in_quote: Option<u8> = None;
    for i in 0..bytes.len() {
        match in_quote {
            Some(q) => {
                if bytes[i] == q {
                    in_quote = None;
                }
            }
            None => match bytes[i] {
                b'"' | b'\'' => in_quote = Some(bytes[i]),
                b'#' if i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t' => {
                    return (&s[..i], &s[i..]);
                }
                _ => {}
            },
        }
    }
    (s, "")
}

fn matches_at(s: &str, i: usize, word: &str) -> bool {
    s[i..].starts_with(word)
}

fn utf8_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Append a token, merging consecutive same-kind tokens so the egui
/// LayoutJob section count stays small.
fn push_tok(out: &mut Vec<(TokKind, String)>, kind: TokKind, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some((last_kind, last_text)) = out.last_mut() {
        if *last_kind == kind {
            last_text.push_str(text);
            return;
        }
    }
    out.push((kind, text.to_string()));
}

// ── Viewer state (mode / selection / pretty toggle) ───────────────────

/// Display mode (WebView `fullscreen.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Outline,
    Raw,
}

/// The window-free state machine behind the data-viewer window.
pub struct DataViewerState {
    pub format: DataFormat,
    /// Original source text (RAW view's non-pretty content).
    pub text: String,
    /// Parse result; `Err` is the banner message.
    pub parsed: Result<Parsed, String>,
    /// Flattened outline rows (empty when parsing failed).
    pub nodes: Vec<TreeNode>,
    /// Selected outline row (index into `nodes`; 0 = root).
    pub selected: usize,
    pub mode: ViewMode,
    /// JSON RAW pretty-print toggle (FR7).
    pub pretty: bool,
    /// Lazily computed pretty-printed JSON (immutable once built).
    pretty_cache: Option<String>,
    /// Lazily computed detail text for the current selection.
    detail_cache: Option<(usize, String)>,
}

impl DataViewerState {
    pub fn new(format: DataFormat, text: String) -> Self {
        let parsed = parse(format, &text);
        let nodes = match &parsed {
            Ok(p) => build_tree(p),
            Err(_) => Vec::new(),
        };
        // FR9: parse errors disable the outline; the viewer opens in RAW.
        let mode = if parsed.is_ok() {
            ViewMode::Outline
        } else {
            ViewMode::Raw
        };
        Self {
            format,
            text,
            parsed,
            nodes,
            selected: 0,
            mode,
            pretty: false,
            pretty_cache: None,
            detail_cache: None,
        }
    }

    pub fn parse_error(&self) -> Option<&str> {
        self.parsed.as_ref().err().map(|s| s.as_str())
    }

    /// `r` — toggle outline ↔ RAW. No-op while the parse failed (FR9).
    pub fn toggle_mode(&mut self) {
        if self.parse_error().is_some() {
            return;
        }
        self.mode = match self.mode {
            ViewMode::Outline => ViewMode::Raw,
            ViewMode::Raw => ViewMode::Outline,
        };
    }

    /// `p` — toggle JSON pretty-print. Only in JSON RAW view (FR7).
    pub fn toggle_pretty(&mut self) {
        if self.mode != ViewMode::Raw
            || self.format != DataFormat::Json
            || self.parse_error().is_some()
        {
            return;
        }
        self.pretty = !self.pretty;
    }

    /// Move the outline selection by `delta` rows, clamped (WebView
    /// `navigateBy`).
    pub fn navigate(&mut self, delta: isize) {
        if self.nodes.is_empty() {
            return;
        }
        let max = self.nodes.len() as isize - 1;
        let next = (self.selected as isize + delta).clamp(0, max);
        self.selected = next as usize;
        self.detail_cache = None;
    }

    /// Select an absolute row (mouse click).
    pub fn select(&mut self, index: usize) {
        if index < self.nodes.len() {
            self.selected = index;
            self.detail_cache = None;
        }
    }

    pub fn select_first(&mut self) {
        self.select(0);
    }

    pub fn select_last(&mut self) {
        if !self.nodes.is_empty() {
            self.select(self.nodes.len() - 1);
        }
    }

    /// Detail-pane text for the current selection (original format,
    /// 2-space indent). Cached per selection.
    pub fn detail(&mut self) -> &str {
        let sel = self.selected;
        let needs = !matches!(&self.detail_cache, Some((s, _)) if *s == sel);
        if needs {
            let text = match (&self.parsed, self.nodes.get(sel)) {
                (Ok(p), Some(node)) => detail_text(p, &node.path),
                _ => String::new(),
            };
            self.detail_cache = Some((sel, text));
        }
        &self.detail_cache.as_ref().expect("filled above").1
    }

    /// RAW-view display text: pretty JSON when toggled on, otherwise the
    /// original source.
    pub fn raw_display_text(&mut self) -> &str {
        if self.pretty && self.format == DataFormat::Json {
            if self.pretty_cache.is_none() {
                self.pretty_cache = self
                    .parsed
                    .as_ref()
                    .ok()
                    .and_then(pretty_json)
                    .or_else(|| Some(self.text.clone()));
            }
            return self.pretty_cache.as_deref().expect("filled above");
        }
        &self.text
    }

    /// Header badge, WebView `fullscreen.ts`: `JSON [Outline]` etc.
    pub fn badge(&self) -> String {
        let fmt = match self.format {
            DataFormat::Json => "JSON",
            DataFormat::Yaml => "YAML",
        };
        let mode = match self.mode {
            ViewMode::Outline => "Outline",
            ViewMode::Raw => "RAW",
        };
        format!("{fmt} [{mode}]")
    }

    /// Footer hints, WebView `fullscreen.ts` parity.
    pub fn footer_hint(&self) -> String {
        if self.parse_error().is_some() {
            return "[Esc] Close".to_string();
        }
        match (self.mode, self.format) {
            (ViewMode::Raw, DataFormat::Json) => {
                let state = if self.pretty { "on" } else { "off" };
                format!("[r] Toggle  [p] Pretty ({state})  [Esc] Close")
            }
            _ => "[r] Toggle  [Esc] Close".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(toks: &[(TokKind, String)]) -> Vec<(TokKind, &str)> {
        toks.iter().map(|(k, s)| (*k, s.as_str())).collect()
    }

    // ── parsing ──────────────────────────────────────────────────────

    #[test]
    fn parse_valid_json_and_yaml() {
        assert!(parse(DataFormat::Json, "{\"a\": 1}").is_ok());
        assert!(parse(DataFormat::Yaml, "a: 1\n").is_ok());
    }

    #[test]
    fn parse_invalid_inputs_return_error_message() {
        let e = parse(DataFormat::Json, "{nope").unwrap_err();
        assert!(!e.is_empty());
        let e = parse(DataFormat::Yaml, "a: [unclosed").unwrap_err();
        assert!(!e.is_empty());
    }

    // ── tree builder ─────────────────────────────────────────────────

    #[test]
    fn tree_flattens_nested_object_in_document_order() {
        let p = parse(DataFormat::Json, r#"{"b": {"x": 1}, "a": [10, 20]}"#).unwrap();
        let nodes = build_tree(&p);
        let labels: Vec<(&str, usize)> =
            nodes.iter().map(|n| (n.label.as_str(), n.depth)).collect();
        // preserve_order: "b" before "a" (insertion order, not sorted).
        assert_eq!(
            labels,
            vec![
                ("(root)", 0),
                ("b", 1),
                ("x", 2),
                ("a", 1),
                ("[0]", 2),
                ("[1]", 2),
            ]
        );
        assert!(nodes[1].has_children);
        assert!(!nodes[2].has_children);
    }

    #[test]
    fn tree_handles_empty_containers() {
        let p = parse(DataFormat::Json, "{}").unwrap();
        let nodes = build_tree(&p);
        assert_eq!(nodes.len(), 1);
        assert!(!nodes[0].has_children);
    }

    #[test]
    fn yaml_tree_includes_sequences_and_non_string_keys() {
        let p = parse(DataFormat::Yaml, "1: one\nlist:\n  - a\n  - b\n").unwrap();
        let nodes = build_tree(&p);
        let labels: Vec<&str> = nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, vec!["(root)", "1", "list", "[0]", "[1]"]);
    }

    // ── detail rendering ─────────────────────────────────────────────

    #[test]
    fn detail_renders_json_subtree_with_two_space_indent() {
        let p = parse(DataFormat::Json, r#"{"a": {"b": 1}}"#).unwrap();
        let text = detail_text(&p, &[PathSeg::Key("a".into())]);
        assert_eq!(text, "{\n  \"b\": 1\n}");
    }

    #[test]
    fn detail_renders_yaml_subtree_in_yaml() {
        let p = parse(DataFormat::Yaml, "a:\n  b: 1\n").unwrap();
        let text = detail_text(&p, &[PathSeg::Key("a".into())]);
        assert_eq!(text.trim_end(), "b: 1");
    }

    #[test]
    fn detail_with_empty_path_renders_whole_document() {
        let p = parse(DataFormat::Json, r#"{"a": 1}"#).unwrap();
        assert_eq!(detail_text(&p, &[]), "{\n  \"a\": 1\n}");
    }

    #[test]
    fn yaml_aliases_resolve_in_detail() {
        let p = parse(DataFormat::Yaml, "base: &b\n  k: 1\nref: *b\n").unwrap();
        let text = detail_text(&p, &[PathSeg::Key("ref".into())]);
        assert_eq!(text.trim_end(), "k: 1");
    }

    // ── highlighter ──────────────────────────────────────────────────

    #[test]
    fn json_line_classifies_key_string_number_bool_null() {
        let toks = highlight_json_line(r#"  "k": "v", "n": 1.5, "b": true, "z": null"#);
        let k = kinds(&toks);
        assert!(k.contains(&(TokKind::Key, "\"k\"")));
        assert!(k.contains(&(TokKind::Str, "\"v\"")));
        assert!(k.contains(&(TokKind::Num, "1.5")));
        assert!(k.contains(&(TokKind::Bool, "true")));
        assert!(k.contains(&(TokKind::Null, "null")));
    }

    #[test]
    fn json_string_with_escaped_quote_and_colon_stays_one_token() {
        let toks = highlight_json_line(r#""a\"b: c""#);
        assert_eq!(kinds(&toks), vec![(TokKind::Str, r#""a\"b: c""#)]);
    }

    #[test]
    fn json_round_trips_concatenation() {
        let line = r#"{"a": [1, true, null, "x"]}"#;
        let joined: String = highlight_json_line(line)
            .iter()
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(joined, line);
    }

    #[test]
    fn yaml_line_classifies_comment_key_and_scalars() {
        assert_eq!(
            kinds(&highlight_yaml_line("# top comment")),
            vec![(TokKind::Comment, "# top comment")]
        );
        let toks = highlight_yaml_line("  count: 42");
        let k = kinds(&toks);
        assert!(k.contains(&(TokKind::Key, "count")));
        assert!(k.contains(&(TokKind::Num, "42")));
        let toks = highlight_yaml_line("flag: true # inline");
        let k = kinds(&toks);
        assert!(k.contains(&(TokKind::Bool, "true")));
        assert!(k.contains(&(TokKind::Comment, "# inline")));
    }

    #[test]
    fn yaml_list_item_marker_is_punct() {
        let toks = highlight_yaml_line("  - hello");
        let k = kinds(&toks);
        assert!(k.contains(&(TokKind::Punct, "- ")));
        assert!(k.contains(&(TokKind::Str, "hello")));
    }

    #[test]
    fn yaml_url_value_does_not_split_at_colon() {
        let toks = highlight_yaml_line("url: http://example.com/x");
        let k = kinds(&toks);
        assert!(k.contains(&(TokKind::Key, "url")));
        assert!(k.contains(&(TokKind::Str, "http://example.com/x")));
    }

    #[test]
    fn yaml_round_trips_concatenation() {
        for line in ["key: value # c", "  - 1", "# only", "plain", "a: 'q: x'"] {
            let joined: String = highlight_yaml_line(line)
                .iter()
                .map(|(_, s)| s.as_str())
                .collect();
            assert_eq!(joined, line, "line {line:?}");
        }
    }

    // ── state machine ────────────────────────────────────────────────

    #[test]
    fn valid_document_opens_in_outline() {
        let s = DataViewerState::new(DataFormat::Json, "{\"a\":1}".into());
        assert_eq!(s.mode, ViewMode::Outline);
        assert!(s.parse_error().is_none());
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn parse_error_opens_in_raw_and_locks_mode() {
        let mut s = DataViewerState::new(DataFormat::Json, "{broken".into());
        assert_eq!(s.mode, ViewMode::Raw);
        assert!(s.parse_error().is_some());
        s.toggle_mode();
        assert_eq!(s.mode, ViewMode::Raw, "r must be a no-op on parse error");
        assert_eq!(s.footer_hint(), "[Esc] Close");
    }

    #[test]
    fn pretty_toggles_only_in_json_raw() {
        let mut s = DataViewerState::new(DataFormat::Json, "{\"a\":1}".into());
        s.toggle_pretty(); // outline → no-op
        assert!(!s.pretty);
        s.toggle_mode(); // → raw
        s.toggle_pretty();
        assert!(s.pretty);
        assert_eq!(s.raw_display_text(), "{\n  \"a\": 1\n}");
        s.toggle_pretty();
        assert_eq!(s.raw_display_text(), "{\"a\":1}");

        let mut y = DataViewerState::new(DataFormat::Yaml, "a: 1\n".into());
        y.toggle_mode();
        y.toggle_pretty(); // YAML → no-op
        assert!(!y.pretty);
    }

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut s = DataViewerState::new(DataFormat::Json, r#"{"a":1,"b":2}"#.into());
        assert_eq!(s.nodes.len(), 3);
        s.navigate(-5);
        assert_eq!(s.selected, 0);
        s.navigate(10);
        assert_eq!(s.selected, 2);
        s.select_first();
        assert_eq!(s.selected, 0);
        s.select_last();
        assert_eq!(s.selected, 2);
    }

    #[test]
    fn detail_follows_selection() {
        let mut s = DataViewerState::new(DataFormat::Json, r#"{"a": {"b": 2}}"#.into());
        assert_eq!(s.detail(), "{\n  \"a\": {\n    \"b\": 2\n  }\n}");
        s.navigate(1); // "a"
        assert_eq!(s.detail(), "{\n  \"b\": 2\n}");
        s.navigate(1); // "b"
        assert_eq!(s.detail(), "2");
    }

    #[test]
    fn badge_and_footer_reflect_mode() {
        let mut s = DataViewerState::new(DataFormat::Yaml, "a: 1\n".into());
        assert_eq!(s.badge(), "YAML [Outline]");
        assert_eq!(s.footer_hint(), "[r] Toggle  [Esc] Close");
        s.toggle_mode();
        assert_eq!(s.badge(), "YAML [RAW]");

        let mut j = DataViewerState::new(DataFormat::Json, "{}".into());
        j.toggle_mode();
        assert_eq!(j.footer_hint(), "[r] Toggle  [p] Pretty (off)  [Esc] Close");
        j.toggle_pretty();
        assert_eq!(j.footer_hint(), "[r] Toggle  [p] Pretty (on)  [Esc] Close");
    }

    #[test]
    fn deeply_nested_structure_builds_tree() {
        // 100+ nesting levels (SPEC edge case).
        let mut doc = String::new();
        for _ in 0..120 {
            doc.push_str("{\"k\":");
        }
        doc.push('1');
        for _ in 0..120 {
            doc.push('}');
        }
        let s = DataViewerState::new(DataFormat::Json, doc);
        // serde_json's default recursion limit (128) accepts 120 levels.
        assert!(s.parse_error().is_none());
        assert_eq!(s.nodes.len(), 121);
        assert_eq!(s.nodes.last().unwrap().depth, 120);
    }
}
