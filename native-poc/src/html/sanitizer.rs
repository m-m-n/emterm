//! HTML sanitizer used by the OSC `777;statusbar` route.
//!
//! The WebView build's `src/status-bar/osc-controller.ts::stripHtmlTags`
//! removes all tags from incoming `set;left;<content>` payloads, and
//! intentionally also removes `<script>` / `<style>` *contents*. This
//! module mirrors that behavior on the native side so an OSC writer
//! cannot inject styled / scripted content via the status-bar layer
//! even though our renderer wouldn't execute it.
//!
//! Note: non-HTML angle brackets in the input (e.g. `1 < 2`) are
//! preserved verbatim. The tokenizer's lenient mode treats them as
//! literal text.

use crate::html::tokenizer::{tokenize, Token};

/// Strip all tags from `input`, dropping `<script>` / `<style>`
/// contents entirely. Returns the concatenated text.
///
/// Examples:
///
/// - `"<b>bold</b>"` → `"bold"`
/// - `"<script>evil()</script>x"` → `"x"`
/// - `"1 < 2"` → `"1 < 2"` (browser-lenient)
pub fn strip_html_tags(input: &str) -> String {
    let tokens = tokenize(input);
    let mut out = String::with_capacity(input.len());
    let mut iter = tokens.into_iter().peekable();
    while let Some(tok) = iter.next() {
        match tok {
            Token::Text(s) => out.push_str(&s),
            Token::TagOpen { name, .. } => {
                if name == "script" || name == "style" {
                    // Drop everything up to and including the matching close.
                    while let Some(next) = iter.next() {
                        if let Token::TagClose { name: close } = &next {
                            if *close == name {
                                break;
                            }
                        }
                    }
                }
                // Other open tags: drop the tag itself; children pass through.
            }
            Token::TagClose { .. } | Token::SelfClosing { .. } => {
                // Drop. `<br>` becomes nothing (not a space) to match
                // the WebView build's behavior.
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_inline_tags_preserving_text() {
        assert_eq!(strip_html_tags("<b>bold</b>"), "bold");
        assert_eq!(strip_html_tags("<span>hi</span>"), "hi");
    }

    #[test]
    fn strip_removes_script_body() {
        assert_eq!(strip_html_tags("a<script>evil()</script>b"), "ab");
    }

    #[test]
    fn strip_removes_style_body() {
        assert_eq!(strip_html_tags("a<style>p{}</style>b"), "ab");
    }

    #[test]
    fn strip_preserves_literal_angle_brackets() {
        assert_eq!(strip_html_tags("1 < 2"), "1 < 2");
    }

    #[test]
    fn strip_handles_entities() {
        assert_eq!(strip_html_tags("&amp;&lt;&gt;"), "&<>");
    }

    #[test]
    fn strip_matches_webview_compound_case() {
        // Mirrors the SPEC acceptance test: mixed text, inline tag,
        // and script run.
        assert_eq!(
            strip_html_tags("1 < 2 <b>bold</b> <script>evil()</script>tail"),
            "1 < 2 bold tail"
        );
    }

    #[test]
    fn strip_empty_input() {
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn strip_nested_tags() {
        assert_eq!(strip_html_tags("<b><i>x</i></b>"), "x");
    }

    #[test]
    fn strip_br_produces_no_separator() {
        // The WebView build's stripHtmlTags drops the tag entirely,
        // with no implicit whitespace insertion. Mirror that.
        assert_eq!(strip_html_tags("a<br>b"), "ab");
    }

    #[test]
    fn strip_keeps_utf8_text() {
        assert_eq!(strip_html_tags("<b>こんにちは</b>"), "こんにちは");
    }
}
