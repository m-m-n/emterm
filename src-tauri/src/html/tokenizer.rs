//! Single-pass HTML tokenizer for the inline-subset parser used by
//! `native-poc`'s status bar (and, in the future, the native Markdown
//! viewer). The tokenizer is intentionally lenient: malformed input
//! falls back to literal text rather than aborting.
//!
//! Output is a stream of [`Token`] values; a higher layer
//! ([`crate::html::parser`]) assembles those into a `Vec<Node>`. The
//! tokenizer does not know about element nesting — it emits
//! `TagOpen` / `TagClose` / `SelfClosing` / `Text` tokens in source
//! order.
//!
//! Reuse policy: keep the surface stable. The Markdown viewer will
//! lean on the same tokenizer to feed a block-aware parser. Extending
//! [`Token`] requires care — prefer adding new fields to existing
//! variants over introducing new top-level shapes that downstream
//! `match` arms must learn to ignore.

use std::collections::HashMap;

/// A single tokenizer output. `Text` collects all characters between
/// markup; `TagOpen` / `TagClose` / `SelfClosing` carry the tag name
/// and (for open tags) the parsed attribute map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Text(String),
    TagOpen {
        name: String,
        attrs: HashMap<String, String>,
    },
    TagClose {
        name: String,
    },
    SelfClosing {
        name: String,
        attrs: HashMap<String, String>,
    },
}

/// Tokenize `input` into a `Vec<Token>`.
///
/// Behavior:
/// - `&lt; &gt; &amp; &quot; &apos;` and numeric entities `&#N;` /
///   `&#xH;` are decoded inside `Text` tokens. Unknown entities are
///   passed through verbatim (`&entity;` survives in the text).
/// - `<` followed by content that does not form a valid tag is
///   emitted as a literal `<` character in the text stream (browser
///   lenient mode). The remainder is rescanned.
/// - Tag names are lowercased before being stored, so consumers can
///   compare case-insensitively without `eq_ignore_ascii_case`.
pub fn tokenize(input: &str) -> Vec<Token> {
    let bytes = input.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        let c = bytes[i];
        if c == b'<' {
            // Try to parse a tag starting here. If successful, flush
            // any pending text first, then push the tag and advance.
            // If parsing fails, emit the literal `<` and resume from
            // the next byte.
            match try_parse_tag(input, i) {
                Some((tok, next)) => {
                    if !text.is_empty() {
                        decode_entities_into(&text, &mut |s| out.push(Token::Text(s.to_string())));
                        text.clear();
                    }
                    out.push(tok);
                    i = next;
                    continue;
                }
                None => {
                    text.push('<');
                    i += 1;
                    continue;
                }
            }
        }
        // Default: accumulate as raw text. Entity decoding happens
        // when the text run is flushed.
        text.push(c as char);
        // Multibyte UTF-8: copy the rest of the codepoint as-is.
        if c >= 0x80 {
            let cp_len = utf8_continuation_len(c);
            if i + 1 < len {
                text.pop(); // remove the placeholder ASCII push
                text.push_str(&input[i..i + cp_len]);
            }
            i += cp_len;
        } else {
            i += 1;
        }
    }
    if !text.is_empty() {
        decode_entities_into(&text, &mut |s| out.push(Token::Text(s.to_string())));
    }
    out
}

/// UTF-8 leading-byte length lookup. Returns 1 for ASCII; 2/3/4 for
/// multibyte leading bytes; 1 as a defensive fallback for invalid
/// leading bytes (the rescanner then treats them as text).
fn utf8_continuation_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

/// Attempt to parse a tag starting at byte offset `start` (which must
/// point at a `<`). Returns the parsed token and the index of the
/// byte just past the closing `>` on success, or `None` on failure
/// (the caller treats the `<` as literal text).
fn try_parse_tag(input: &str, start: usize) -> Option<(Token, usize)> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    debug_assert_eq!(bytes[start], b'<');
    if start + 1 >= len {
        return None;
    }
    let mut i = start + 1;
    let is_close = if bytes[i] == b'/' {
        i += 1;
        true
    } else {
        false
    };
    // Parse the tag name (ASCII alphanumeric + `-`).
    let name_start = i;
    while i < len && is_name_byte(bytes[i]) {
        i += 1;
    }
    if i == name_start {
        return None; // No name — not a tag.
    }
    let name = input[name_start..i].to_ascii_lowercase();

    // Skip whitespace.
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    // Parse attributes (only on open tags; close tags MAY have whitespace
    // but no attributes per HTML5 — we tolerate and discard junk).
    let mut attrs: HashMap<String, String> = HashMap::new();
    let mut self_closing = false;
    while i < len {
        let c = bytes[i];
        if c == b'>' {
            i += 1;
            break;
        }
        if c == b'/' {
            // `<br/>` style. The `/` is allowed before `>`; we mark the
            // tag self-closing and look for the immediate `>`.
            self_closing = true;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if is_close {
            // Close tags don't carry attributes; skip junk until `>`.
            while i < len && bytes[i] != b'>' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            break;
        }
        // Parse one attribute.
        let attr_name_start = i;
        while i < len
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let attr_name = input[attr_name_start..i].to_ascii_lowercase();
        // Skip whitespace before `=`.
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let value: String = if i < len && bytes[i] == b'=' {
            i += 1;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < len && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < len && bytes[i] != quote {
                    i += 1;
                }
                if i >= len {
                    return None; // Unterminated quote → not a tag.
                }
                let raw = &input[value_start..i];
                i += 1; // skip closing quote
                decode_entities_value(raw)
            } else {
                let value_start = i;
                // HTML5 unquoted values terminate on whitespace or `>`.
                // `/` is allowed inside unquoted values (so URLs survive).
                while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                let raw = &input[value_start..i];
                decode_entities_value(raw)
            }
        } else {
            String::new()
        };
        if !attr_name.is_empty() {
            attrs.insert(attr_name, value);
        }
    }

    if is_close {
        Some((Token::TagClose { name }, i))
    } else if self_closing || is_void_element(&name) {
        Some((Token::SelfClosing { name, attrs }, i))
    } else {
        Some((Token::TagOpen { name, attrs }, i))
    }
}

/// `true` for ASCII bytes valid inside a tag/attribute name. We are
/// liberal — tag names in our subset stay alphanumeric — but accept
/// the broader HTML5 set to keep the lenient mode predictable.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':'
}

/// HTML5 void elements that never have content. We treat `<br>` as
/// self-closing without requiring `/>`. The list is small intentionally;
/// the parser layer will fall through for anything we don't recognise.
fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "br" | "hr" | "img" | "input" | "meta" | "link" | "wbr" | "area" | "base" | "col" | "embed"
    )
}

/// Decode HTML entities in attribute value strings. Used only when
/// flushing attribute values out of the tokenizer.
fn decode_entities_value(input: &str) -> String {
    let mut out = String::new();
    decode_entities_into(input, &mut |s| out.push_str(s));
    out
}

/// Decode HTML entities in `input`, pushing each chunk into `sink`.
/// The sink is invoked with raw `&str` slices in arrival order so the
/// tokenizer can batch them into a single `Text` token without
/// allocating extra intermediate `String`s.
fn decode_entities_into(input: &str, sink: &mut dyn FnMut(&str)) {
    let bytes = input.as_bytes();
    let mut i = 0;
    let len = bytes.len();
    let mut last_flush = 0usize;
    while i < len {
        if bytes[i] != b'&' {
            i += 1;
            continue;
        }
        // Look for terminating `;` within a reasonable window (≤ 10 bytes).
        // HTML5 entities are bounded; restricting the scan keeps us O(n).
        let max = (i + 12).min(len);
        let mut end: Option<usize> = None;
        for j in (i + 1)..max {
            if bytes[j] == b';' {
                end = Some(j);
                break;
            }
            // Spaces / `<` cancel entity scanning — pass `&` through.
            if bytes[j].is_ascii_whitespace() || bytes[j] == b'<' {
                break;
            }
        }
        let Some(end) = end else {
            i += 1;
            continue;
        };
        let entity = &input[i + 1..end];
        if let Some(decoded) = decode_named_or_numeric(entity) {
            if i > last_flush {
                sink(&input[last_flush..i]);
            }
            // SAFETY: decoded is owned by the closure scope; we leak a
            // small buffer into the sink via a temporary string. The
            // sink is `&mut dyn FnMut(&str)` so the borrow lifetime
            // ends before the next iteration.
            // (Internally we copy from a stack-local because the
            // returned `decoded` is a String.)
            sink(&decoded);
            i = end + 1;
            last_flush = i;
        } else {
            i = end + 1;
        }
    }
    if last_flush < len {
        sink(&input[last_flush..]);
    }
}

/// Decode a single entity body (the text between `&` and `;`). Named
/// entities cover the five canonical XML entities plus the most common
/// HTML ones used in status-bar / Markdown content; numeric entities
/// cover `#N` and `#xH`. Returns `None` for unknown forms so the caller
/// can leave the original `&entity;` in place.
fn decode_named_or_numeric(body: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    if body.as_bytes()[0] == b'#' {
        let rest = &body[1..];
        let cp = if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse::<u32>().ok()?
        };
        let ch = char::from_u32(cp)?;
        let mut s = String::new();
        s.push(ch);
        return Some(s);
    }
    let s = match body {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{00A0}",
        _ => return None,
    };
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_plain_text() {
        let toks = tokenize("hello world");
        assert_eq!(toks, vec![Token::Text("hello world".to_string())]);
    }

    #[test]
    fn tokenize_open_close_tag() {
        let toks = tokenize("<b>hi</b>");
        assert_eq!(toks.len(), 3);
        assert!(matches!(&toks[0], Token::TagOpen { name, .. } if name == "b"));
        assert_eq!(toks[1], Token::Text("hi".to_string()));
        assert!(matches!(&toks[2], Token::TagClose { name } if name == "b"));
    }

    #[test]
    fn tokenize_self_closing_br() {
        // Void element <br> is treated as self-closing regardless of /.
        let toks = tokenize("a<br>b<br/>c");
        let kinds: Vec<&str> = toks
            .iter()
            .map(|t| match t {
                Token::Text(_) => "text",
                Token::TagOpen { .. } => "open",
                Token::TagClose { .. } => "close",
                Token::SelfClosing { .. } => "selfclose",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["text", "selfclose", "text", "selfclose", "text"]
        );
    }

    #[test]
    fn tokenize_attribute_with_quoted_value() {
        let toks = tokenize(r#"<span style="color:red">x</span>"#);
        let Token::TagOpen { name, attrs } = &toks[0] else {
            panic!("expected open: {:?}", toks);
        };
        assert_eq!(name, "span");
        assert_eq!(attrs.get("style").map(String::as_str), Some("color:red"));
    }

    #[test]
    fn tokenize_attribute_without_quotes() {
        let toks = tokenize("<a href=https://example.com>x</a>");
        let Token::TagOpen { attrs, .. } = &toks[0] else {
            panic!()
        };
        assert_eq!(
            attrs.get("href").map(String::as_str),
            Some("https://example.com")
        );
    }

    #[test]
    fn tokenize_named_entities_in_text() {
        let toks = tokenize("&amp;&lt;&gt;&quot;&apos;");
        // All five collapse into a single Text token (sink batches them).
        let joined: String = toks
            .iter()
            .map(|t| match t {
                Token::Text(s) => s.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(joined, "&<>\"'");
    }

    #[test]
    fn tokenize_numeric_entity_decimal_and_hex() {
        let toks = tokenize("&#65;&#x42;");
        let joined: String = toks
            .iter()
            .map(|t| match t {
                Token::Text(s) => s.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(joined, "AB");
    }

    #[test]
    fn tokenize_unknown_entity_passes_through() {
        let toks = tokenize("a &fake; b");
        let joined: String = toks
            .iter()
            .map(|t| match t {
                Token::Text(s) => s.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(joined, "a &fake; b");
    }

    #[test]
    fn tokenize_literal_less_than_is_kept_when_not_a_tag() {
        let toks = tokenize("1 < 2");
        let joined: String = toks
            .iter()
            .map(|t| match t {
                Token::Text(s) => s.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(joined, "1 < 2");
    }

    #[test]
    fn tokenize_uppercase_tag_name_is_lowercased() {
        let toks = tokenize("<B>x</B>");
        let names: Vec<&str> = toks
            .iter()
            .filter_map(|t| match t {
                Token::TagOpen { name, .. } | Token::TagClose { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["b", "b"]);
    }

    #[test]
    fn tokenize_utf8_text_round_trip() {
        let toks = tokenize("こんにちは <b>世界</b>");
        let joined: String = toks
            .iter()
            .map(|t| match t {
                Token::Text(s) => s.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(joined, "こんにちは 世界");
    }
}
