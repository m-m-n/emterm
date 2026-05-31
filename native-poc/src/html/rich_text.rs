//! AST → flat run-list conversion.
//!
//! egui's `RichText` widget styles per-label only; to render mixed
//! styled text we emit a `Vec<RichTextRun>` that the caller draws as
//! a horizontal sequence of `Label`s. Each run carries the resolved
//! style attributes (the AST's nesting has been flattened by AND
//! over bold/italic/underline, with the innermost `color` winning).

use crate::html::parser::{CssColor, Node};

/// A single styled text run ready for `ui.label(RichText::new(...))`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichTextRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<CssColor>,
    /// `true` when this run represents a `<br>` linebreak with no text.
    /// Renderers should insert a soft line break before drawing the next
    /// run. The `text` field is empty in this case.
    pub line_break: bool,
}

impl RichTextRun {
    fn from_text(text: String, style: Style) -> Self {
        Self {
            text,
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            color: style.color,
            line_break: false,
        }
    }
    fn line_break() -> Self {
        Self {
            text: String::new(),
            bold: false,
            italic: false,
            underline: false,
            color: None,
            line_break: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    color: Option<CssColor>,
}

/// Flatten a `Vec<Node>` into a `Vec<RichTextRun>`. Adjacent runs with
/// identical styling are merged so the renderer issues fewer labels.
pub fn to_rich_text_runs(nodes: &[Node]) -> Vec<RichTextRun> {
    let mut out: Vec<RichTextRun> = Vec::new();
    let style = Style::default();
    visit(nodes, &style, &mut out);
    // Merge contiguous identical-style runs.
    let mut merged: Vec<RichTextRun> = Vec::with_capacity(out.len());
    for run in out {
        match merged.last_mut() {
            Some(last)
                if !last.line_break
                    && !run.line_break
                    && last.bold == run.bold
                    && last.italic == run.italic
                    && last.underline == run.underline
                    && last.color == run.color =>
            {
                last.text.push_str(&run.text);
            }
            _ => merged.push(run),
        }
    }
    merged
}

fn visit(nodes: &[Node], style: &Style, out: &mut Vec<RichTextRun>) {
    for node in nodes {
        match node {
            Node::Text(t) => {
                if !t.is_empty() {
                    out.push(RichTextRun::from_text(t.clone(), style.clone()));
                }
            }
            Node::LineBreak => {
                out.push(RichTextRun::line_break());
            }
            Node::Bold(children) => {
                let mut s = style.clone();
                s.bold = true;
                visit(children, &s, out);
            }
            Node::Italic(children) => {
                let mut s = style.clone();
                s.italic = true;
                visit(children, &s, out);
            }
            Node::Underline(children) => {
                let mut s = style.clone();
                s.underline = true;
                visit(children, &s, out);
            }
            Node::Span { color, children } => {
                let mut s = style.clone();
                // Inner color wins. (`Option::or` keeps the existing
                // `Some` if present.)
                if let Some(c) = color {
                    s.color = Some(c.clone());
                }
                visit(children, &s, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::parser::parse;

    #[test]
    fn flatten_plain_text() {
        let runs = to_rich_text_runs(&parse("hello"));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "hello");
        assert!(!runs[0].bold);
    }

    #[test]
    fn flatten_bold_italic_nested() {
        let runs = to_rich_text_runs(&parse("<b><i>x</i></b>"));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "x");
        assert!(runs[0].bold);
        assert!(runs[0].italic);
    }

    #[test]
    fn flatten_span_color_and_text() {
        let runs = to_rich_text_runs(&parse(r#"a<span style="color:red">b</span>c"#));
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text, "a");
        assert!(runs[0].color.is_none());
        assert_eq!(runs[1].text, "b");
        assert_eq!(runs[1].color, Some(CssColor::Named("red".to_string())));
        assert_eq!(runs[2].text, "c");
        assert!(runs[2].color.is_none());
    }

    #[test]
    fn flatten_br_produces_line_break_run() {
        let runs = to_rich_text_runs(&parse("a<br>b"));
        assert_eq!(runs.len(), 3);
        assert!(runs[1].line_break);
        assert_eq!(runs[1].text, "");
    }

    #[test]
    fn flatten_merges_adjacent_identical_style() {
        // Two text nodes with identical styling merge into one run.
        let runs = to_rich_text_runs(&[Node::Bold(vec![
            Node::Text("aa".to_string()),
            Node::Text("bb".to_string()),
        ])]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "aabb");
        assert!(runs[0].bold);
    }

    #[test]
    fn flatten_font_color_produces_colored_run() {
        // End-to-end: `<font color>` flows parse → flatten and carries
        // its color through to the run list (the status-bar color path).
        let runs = to_rich_text_runs(&parse(r#"CO2: <font color="limegreen">594</font>ppm"#));
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text, "CO2: ");
        assert!(runs[0].color.is_none());
        assert_eq!(runs[1].text, "594");
        assert_eq!(
            runs[1].color,
            Some(CssColor::Named("limegreen".to_string()))
        );
        assert_eq!(runs[2].text, "ppm");
        assert!(runs[2].color.is_none());
    }

    #[test]
    fn flatten_inner_color_wins_over_outer() {
        let runs = to_rich_text_runs(&parse(
            r#"<span style="color:red"><span style="color:blue">x</span></span>"#,
        ));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].color, Some(CssColor::Named("blue".to_string())));
    }
}
