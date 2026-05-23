//! Stack-based HTML parser. Consumes tokens from
//! [`crate::html::tokenizer::tokenize`] and produces a `Vec<Node>`
//! tree for downstream conversion (RichText runs, future Markdown
//! viewer block list).
//!
//! Design notes:
//! - The parser is **lenient**. Unknown elements survive as
//!   transparent wrappers (their children are inlined into the
//!   parent) so unsupported markup never strips content.
//! - `<script>` and `<style>` are special: the parser swallows their
//!   children entirely. The OSC route already strips tags with
//!   `strip_html_tags`, but the parser also enforces removal at the
//!   AST layer so a future code path that feeds untrusted HTML in
//!   without sanitizing cannot leak script bodies.
//! - The [`Node`] enum is `#[non_exhaustive]` so adding block-level
//!   variants for the future Markdown viewer port does not break
//!   downstream `match` arms.

use std::collections::HashMap;

use crate::html::tokenizer::{tokenize, Token};

/// Color value as it appears in a CSS color expression. We keep the
/// parsed form so the egui conversion layer can produce a
/// `Color32` without re-parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CssColor {
    /// `#RGB` / `#RRGGBB`.
    Hex { r: u8, g: u8, b: u8 },
    /// `rgb(r, g, b)`.
    Rgb { r: u8, g: u8, b: u8 },
    /// Named color keyword (e.g. `red`). Lowercased.
    Named(String),
}

/// Inline AST node. Block-level shapes are deliberately omitted —
/// they land in a follow-up phase when the Markdown viewer port
/// reuses this module. `#[non_exhaustive]` lets us add them without
/// breaking exhaustive `match` arms in current callers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Node {
    Text(String),
    LineBreak,
    Span {
        color: Option<CssColor>,
        children: Vec<Node>,
    },
    Bold(Vec<Node>),
    Italic(Vec<Node>),
    Underline(Vec<Node>),
}

/// Parse `input` into a `Vec<Node>`. Always succeeds (lenient mode).
pub fn parse(input: &str) -> Vec<Node> {
    let tokens = tokenize(input);
    parse_tokens(&tokens)
}

/// Internal entry point used by tests + sanitizer.
pub(crate) fn parse_tokens(tokens: &[Token]) -> Vec<Node> {
    let mut iter = tokens.iter().peekable();
    parse_children(&mut iter, None)
}

/// Recursively parse until either the input is exhausted or a
/// closing tag matching `stop_at` is observed. `stop_at == None`
/// means the top-level call (terminate on EOF).
fn parse_children<'a, I>(iter: &mut std::iter::Peekable<I>, stop_at: Option<&str>) -> Vec<Node>
where
    I: Iterator<Item = &'a Token>,
{
    let mut out: Vec<Node> = Vec::new();
    while let Some(tok) = iter.peek() {
        match tok {
            Token::TagClose { name } => {
                if let Some(stop) = stop_at {
                    if name == stop {
                        // Consume the close tag and return.
                        iter.next();
                        return out;
                    }
                }
                // Mismatched close: drop the tag entirely (lenient).
                log::warn!("html parser: unmatched </{name}>; dropping");
                iter.next();
            }
            Token::TagOpen { name, attrs } => {
                let name = name.clone();
                let attrs = attrs.clone();
                iter.next();
                match name.as_str() {
                    "script" | "style" => {
                        // Swallow content until matching close tag.
                        consume_until_close(iter, &name);
                    }
                    "b" | "strong" => {
                        let children = parse_children(iter, Some(&name));
                        out.push(Node::Bold(children));
                    }
                    "i" | "em" => {
                        let children = parse_children(iter, Some(&name));
                        out.push(Node::Italic(children));
                    }
                    "u" => {
                        let children = parse_children(iter, Some("u"));
                        out.push(Node::Underline(children));
                    }
                    "span" => {
                        let color = extract_color_from_style(&attrs);
                        let children = parse_children(iter, Some("span"));
                        out.push(Node::Span { color, children });
                    }
                    _ => {
                        // Unknown tag: emit children as-is (transparent).
                        let children = parse_children(iter, Some(&name));
                        out.extend(children);
                    }
                }
            }
            Token::SelfClosing { name, .. } => {
                let name = name.clone();
                iter.next();
                if name == "br" {
                    out.push(Node::LineBreak);
                }
                // Other self-closing tags (img, etc.) currently
                // produce nothing; the Markdown viewer port will add
                // dedicated variants.
            }
            Token::Text(s) => {
                let s = s.clone();
                iter.next();
                if !s.is_empty() {
                    out.push(Node::Text(s));
                }
            }
        }
    }
    out
}

/// Skip tokens until the matching `</name>` close is consumed. Used
/// for `<script>` / `<style>` content removal.
fn consume_until_close<'a, I>(iter: &mut std::iter::Peekable<I>, name: &str)
where
    I: Iterator<Item = &'a Token>,
{
    while let Some(tok) = iter.next() {
        if let Token::TagClose { name: close } = tok {
            if close == name {
                return;
            }
        }
    }
}

/// Parse a CSS color from a `style="..."` attribute. Returns `None`
/// when the attribute is missing, or when no `color:` declaration is
/// present, or when the value is not understood.
fn extract_color_from_style(attrs: &HashMap<String, String>) -> Option<CssColor> {
    let style = attrs.get("style")?;
    for decl in style.split(';') {
        let mut parts = decl.splitn(2, ':');
        let prop = parts.next()?.trim().to_ascii_lowercase();
        let value = parts.next()?.trim();
        if prop == "color" {
            return parse_css_color(value);
        }
    }
    None
}

/// Parse a CSS color literal. Supports `#RGB`, `#RRGGBB`,
/// `rgb(r, g, b)` (decimals or percentages), and named keywords.
pub fn parse_css_color(value: &str) -> Option<CssColor> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('#') {
        return parse_hex_color(rest).map(|(r, g, b)| CssColor::Hex { r, g, b });
    }
    if let Some(rest) = value.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        return parse_rgb_components(rest).map(|(r, g, b)| CssColor::Rgb { r, g, b });
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == '-' || c == '_')
        && !value.is_empty()
    {
        return Some(CssColor::Named(value.to_ascii_lowercase()));
    }
    None
}

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn parse_rgb_components(input: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = input.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    let comp = |s: &str| -> Option<u8> {
        if let Some(num) = s.strip_suffix('%') {
            let v: f32 = num.parse().ok()?;
            Some((v.clamp(0.0, 100.0) * 2.55).round() as u8)
        } else {
            let v: u32 = s.parse().ok()?;
            Some(v.min(255) as u8)
        }
    };
    Some((comp(parts[0])?, comp(parts[1])?, comp(parts[2])?))
}

impl CssColor {
    /// Convert to an egui `Color32`. Named colors fall back to a small
    /// table of common CSS basics; unknown names return `None` so the
    /// caller can leave the theme color alone.
    pub fn to_egui(&self) -> Option<egui::Color32> {
        match self {
            CssColor::Hex { r, g, b } | CssColor::Rgb { r, g, b } => {
                Some(egui::Color32::from_rgb(*r, *g, *b))
            }
            CssColor::Named(name) => named_css_color(name),
        }
    }
}

fn named_css_color(name: &str) -> Option<egui::Color32> {
    // Subset that covers the WebView build's status-bar palette plus
    // the handful of names commonly used in OSC payloads.
    let (r, g, b) = match name {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "pink" => (255, 192, 203),
        "lime" => (0, 255, 0),
        "navy" => (0, 0, 128),
        "teal" => (0, 128, 128),
        "olive" => (128, 128, 0),
        "maroon" => (128, 0, 0),
        "silver" => (192, 192, 192),
        _ => return None,
    };
    Some(egui::Color32::from_rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_text() {
        let nodes = parse("hello");
        assert_eq!(nodes, vec![Node::Text("hello".to_string())]);
    }

    #[test]
    fn parse_bold_inline() {
        let nodes = parse("<b>x</b>");
        assert_eq!(nodes, vec![Node::Bold(vec![Node::Text("x".to_string())])]);
    }

    #[test]
    fn parse_nested_bold_italic() {
        let nodes = parse("<b><i>x</i></b>");
        assert_eq!(
            nodes,
            vec![Node::Bold(vec![Node::Italic(vec![Node::Text(
                "x".to_string()
            )])])]
        );
    }

    #[test]
    fn parse_span_with_color_hex() {
        let nodes = parse(r#"<span style="color:#ff0000">x</span>"#);
        assert_eq!(
            nodes,
            vec![Node::Span {
                color: Some(CssColor::Hex {
                    r: 0xff,
                    g: 0,
                    b: 0
                }),
                children: vec![Node::Text("x".to_string())],
            }]
        );
    }

    #[test]
    fn parse_span_with_color_named() {
        let nodes = parse(r#"<span style="color:red">x</span>"#);
        let Node::Span { color, .. } = &nodes[0] else {
            panic!()
        };
        assert_eq!(color, &Some(CssColor::Named("red".to_string())));
    }

    #[test]
    fn parse_span_with_color_rgb() {
        let nodes = parse(r#"<span style="color: rgb(10, 20, 30)">x</span>"#);
        let Node::Span { color, .. } = &nodes[0] else {
            panic!()
        };
        assert_eq!(
            color,
            &Some(CssColor::Rgb {
                r: 10,
                g: 20,
                b: 30
            })
        );
    }

    #[test]
    fn parse_br_is_linebreak() {
        let nodes = parse("a<br>b");
        assert_eq!(
            nodes,
            vec![
                Node::Text("a".to_string()),
                Node::LineBreak,
                Node::Text("b".to_string()),
            ]
        );
    }

    #[test]
    fn parse_unknown_tag_emits_children_only() {
        let nodes = parse("<unknown>x</unknown>");
        assert_eq!(nodes, vec![Node::Text("x".to_string())]);
    }

    #[test]
    fn parse_script_content_is_dropped() {
        let nodes = parse("a<script>evil()</script>b");
        assert_eq!(
            nodes,
            vec![Node::Text("a".to_string()), Node::Text("b".to_string()),]
        );
    }

    #[test]
    fn parse_style_content_is_dropped() {
        let nodes = parse("a<style>p{color:red}</style>b");
        assert_eq!(
            nodes,
            vec![Node::Text("a".to_string()), Node::Text("b".to_string()),]
        );
    }

    #[test]
    fn parse_mismatched_close_is_dropped() {
        // `</b>` with no matching open: ignored.
        let nodes = parse("hello</b>world");
        assert_eq!(
            nodes,
            vec![
                Node::Text("hello".to_string()),
                Node::Text("world".to_string())
            ]
        );
    }

    #[test]
    fn parse_strong_maps_to_bold() {
        let nodes = parse("<strong>x</strong>");
        assert_eq!(nodes, vec![Node::Bold(vec![Node::Text("x".to_string())])]);
    }

    #[test]
    fn parse_em_maps_to_italic() {
        let nodes = parse("<em>x</em>");
        assert_eq!(nodes, vec![Node::Italic(vec![Node::Text("x".to_string())])]);
    }

    #[test]
    fn parse_u_maps_to_underline() {
        let nodes = parse("<u>x</u>");
        assert_eq!(
            nodes,
            vec![Node::Underline(vec![Node::Text("x".to_string())])]
        );
    }

    #[test]
    fn parse_css_color_hex_short() {
        assert_eq!(
            parse_css_color("#abc"),
            Some(CssColor::Hex {
                r: 0xaa,
                g: 0xbb,
                b: 0xcc,
            })
        );
    }

    #[test]
    fn parse_css_color_invalid_returns_none() {
        assert_eq!(parse_css_color("not a color 1"), None);
    }

    #[test]
    fn css_color_to_egui_named_red() {
        let c = CssColor::Named("red".to_string());
        assert_eq!(c.to_egui(), Some(egui::Color32::from_rgb(255, 0, 0)));
    }

    #[test]
    fn css_color_to_egui_named_unknown_returns_none() {
        let c = CssColor::Named("not-a-color".to_string());
        assert_eq!(c.to_egui(), None);
    }
}
