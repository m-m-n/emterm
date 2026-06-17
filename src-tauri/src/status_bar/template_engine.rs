//! Template variable engine.
//!
//! The engine resolves substrings of the form `{name}` and
//! `{name:argument}` against a registry of [`VariableProvider`]
//! implementations. Unknown variables are replaced with the empty
//! string. The resolved output is an HTML fragment — providers that
//! supply a [`CssColor`] wrap their value in
//! `<span style="color:#rrggbb">value</span>` so the downstream
//! `html::parse` pass turns it into a styled run.
//!
//! Design points:
//! - Single-pass handwritten scanner. No `regex` crate.
//! - `VariableProvider` is `Send + Sync`. Providers that need IO run
//!   it on their own worker thread; `get_value()` returns the most
//!   recent cached value.
//! - The engine is shared between the per-frame `build_view_model`
//!   path and tests, so locking inside providers must be short.
//! - Provider-supplied values are HTML-escaped before being embedded
//!   into the output fragment so an upstream `{cmd:foo}` that returns
//!   `<script>...` or `<b>...` cannot smuggle its own markup through
//!   the downstream `html::parse` pass.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::html::{parse_css_color, tokenize, CssColor, Token};

/// Provider trait. Implementations return cached values; any IO
/// happens out-of-band on a worker thread.
pub trait VariableProvider: Send + Sync {
    /// Variable name as it appears between `{` and `}` (without the
    /// optional `:argument` suffix). Lowercase ASCII recommended.
    fn name(&self) -> &str;

    /// Resolve the current value. `argument` carries the substring
    /// after the first `:` (if any), e.g. `{cmd:my_status}` →
    /// `argument = Some("my_status")`.
    fn get_value(&self, argument: Option<&str>) -> String;

    /// Optional color hint, applied as a CSS color wrapper around the
    /// substituted value. Providers without a coloring concept return
    /// `None`.
    fn get_color(&self, _argument: Option<&str>) -> Option<CssColor> {
        None
    }

    /// Monotonic version counter. Phase F's per-row cache uses this
    /// to invalidate cached `RichTextRun` lists. The default impl
    /// returns 0; provider implementations may override.
    fn version(&self, _argument: Option<&str>) -> u64 {
        0
    }
}

/// Registry of providers keyed by `name()`. Cheap to clone via
/// `Arc` so the runtime can share the same engine with workers.
#[derive(Default)]
pub struct TemplateEngine {
    providers: HashMap<String, Arc<dyn VariableProvider>>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider. Replaces any provider already registered
    /// under the same name.
    pub fn register(&mut self, provider: Arc<dyn VariableProvider>) {
        let key = provider.name().to_string();
        self.providers.insert(key, provider);
    }

    /// Look up a provider by name. Mostly used by the per-frame cache
    /// key builder.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn VariableProvider>> {
        self.providers.get(name)
    }

    /// Resolve all `{name}` / `{name:arg}` references in `template`,
    /// producing an HTML fragment ready for `html::parse`.
    pub fn resolve(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len());
        let bytes = template.as_bytes();
        let mut i = 0;
        let len = bytes.len();
        while i < len {
            if bytes[i] == b'{' {
                if let Some((name, argument, end)) = parse_variable(template, i) {
                    if let Some(provider) = self.providers.get(name) {
                        let value = provider.get_value(argument);
                        let color = provider.get_color(argument);
                        push_styled(&mut out, &value, color.as_ref());
                    }
                    // else: unknown variable → empty replacement
                    i = end;
                    continue;
                }
            }
            // Copy a UTF-8 codepoint verbatim.
            let cp_len = utf8_len(bytes[i]);
            out.push_str(&template[i..(i + cp_len).min(len)]);
            i += cp_len;
        }
        out
    }

    /// Extract every variable reference present in `template`.
    /// Returns `(name, argument)` tuples in source order, with
    /// duplicates. Used by Phase F's run-list cache to build version
    /// tuples.
    pub fn extract_variables(template: &str) -> Vec<(String, Option<String>)> {
        let mut out: Vec<(String, Option<String>)> = Vec::new();
        let bytes = template.as_bytes();
        let mut i = 0;
        let len = bytes.len();
        while i < len {
            if bytes[i] == b'{' {
                if let Some((name, argument, end)) = parse_variable(template, i) {
                    out.push((name.to_string(), argument.map(str::to_string)));
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
        out
    }
}

/// Try to parse a `{name}` or `{name:arg}` block starting at `start`
/// (which must point at `{`). Returns `(name, argument, end_after_})`
/// or `None` on a malformed block (rolled back to literal `{`).
fn parse_variable(input: &str, start: usize) -> Option<(&str, Option<&str>, usize)> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    debug_assert_eq!(bytes[start], b'{');
    let mut i = start + 1;
    if i >= len {
        return None;
    }
    // Name: ASCII letter / `_` followed by letters / digits / `_`.
    if !is_name_start(bytes[i]) {
        return None;
    }
    let name_start = i;
    i += 1;
    while i < len && is_name_part(bytes[i]) {
        i += 1;
    }
    let name_end = i;
    let mut argument_range: Option<(usize, usize)> = None;
    if i < len && bytes[i] == b':' {
        i += 1;
        let arg_start = i;
        while i < len && is_arg_part(bytes[i]) {
            i += 1;
        }
        if i == arg_start {
            return None;
        }
        argument_range = Some((arg_start, i));
    }
    if i >= len || bytes[i] != b'}' {
        return None;
    }
    let name = &input[name_start..name_end];
    let argument = argument_range.map(|(a, b)| &input[a..b]);
    Some((name, argument, i + 1))
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}
fn is_name_part(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
fn is_arg_part(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn utf8_len(b: u8) -> usize {
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

/// Push the substituted provider value into `out`, honouring the
/// status-bar color markup `<font color="…">…</font>` (see
/// `project_native_status_bar_color_markup`).
///
/// The value is sanitized by [`sanitize_provider_value`]: only `<font>`
/// tags survive (with a re-validated `color` attribute), every other
/// tag is dropped, and all text is HTML-escaped. So a `{cmd:foo}` that
/// prints `<font color="limegreen">594</font>` colors `594`, while one
/// that prints `<b>x</b>` or `<script>…</script>` cannot inject bold,
/// styling, or scripts through the downstream `html::parse` pass.
///
/// When the provider also supplies a `color` hint via `get_color`
/// (e.g. `git_branch`), the sanitized value is wrapped in an outer
/// `<font color="…">` so the whole substitution is colored.
fn push_styled(out: &mut String, value: &str, color: Option<&CssColor>) {
    let sanitized = sanitize_provider_value(value);
    if let Some(color) = color {
        let css = css_color_string(color);
        out.push_str(&format!(r#"<font color="{css}">{sanitized}</font>"#));
    } else {
        out.push_str(&sanitized);
    }
}

/// Sanitize a provider value into the restricted markup the status bar
/// accepts: `<font color="…">…</font>` plus plain (escaped) text.
///
/// - `<font>` open/close tags survive. The `color` attribute is
///   re-validated through [`parse_css_color`] and re-serialized from the
///   parsed value, so a crafted attribute (e.g. `color='x"><script>'`)
///   cannot break out of the tag. A `<font>` whose `color` is missing
///   or unparseable is dropped (its text children still survive).
/// - Every other tag (`<b>`, `<span>`, `<script>`, …) is dropped so a
///   `{cmd}` cannot smuggle styling or scripts past this boundary.
/// - All text is HTML-escaped so the downstream `html::parse` pass sees
///   literal characters, not markup.
fn sanitize_provider_value(value: &str) -> String {
    // Fast path: no markup at all. Most provider values are plain text
    // (clock, hostname, load average), so avoid tokenizing them.
    if !value.contains('<') {
        return escape_html(value).into_owned();
    }
    let mut out = String::with_capacity(value.len());
    // Track, per open `<font>`, whether we actually emitted an opening
    // tag, so a matching `</font>` is emitted only when its open was —
    // keeping the output balanced (a `<font>` with an unusable color
    // emits neither tag, so its close must be suppressed too).
    let mut font_stack: Vec<bool> = Vec::new();
    for tok in tokenize(value) {
        match tok {
            Token::Text(s) => out.push_str(&escape_html(&s)),
            Token::TagOpen { name, attrs } if name == "font" => {
                match attrs.get("color").and_then(|c| parse_css_color(c)) {
                    Some(color) => {
                        out.push_str(&format!(r#"<font color="{}">"#, css_color_string(&color)));
                        font_stack.push(true);
                    }
                    // A `<font>` without a usable color contributes no
                    // tag; its inner text still flows through.
                    None => font_stack.push(false),
                }
            }
            Token::TagClose { name } if name == "font" => {
                // Emit the close only if its matching open was emitted.
                // A stray `</font>` (no open) is dropped.
                if font_stack.pop() == Some(true) {
                    out.push_str("</font>");
                }
            }
            // Drop every other tag (and self-closing `<font/>`, which is
            // nonsensical for coloring).
            _ => {}
        }
    }
    // Close any `<font>` the provider opened but left unclosed. Without
    // this, a `{cmd}` emitting malformed markup (e.g. `<font color="red">x`
    // with no close) yields a dangling open tag; the downstream lenient
    // `html::parse` then extends that color to EOF, bleeding it across the
    // rest of the status-bar line (and later substitutions). Emit one
    // `</font>` per still-open emitted tag so the fragment is balanced.
    for emitted in font_stack.into_iter().rev() {
        if emitted {
            out.push_str("</font>");
        }
    }
    out
}

/// HTML-escape `&`, `<`, `>`, `"`, and `'`. Returns a borrow when
/// the input is already escape-clean to avoid an allocation on the
/// hot path (most provider values are plain ASCII like `"main"` or
/// `"12:34:56"`).
fn escape_html(input: &str) -> Cow<'_, str> {
    if !input
        .bytes()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''))
    {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

fn css_color_string(color: &CssColor) -> String {
    match color {
        CssColor::Hex { r, g, b } | CssColor::Rgb { r, g, b } => {
            format!("#{:02x}{:02x}{:02x}", r, g, b)
        }
        CssColor::Named(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticProvider {
        name: String,
        value: String,
        color: Option<CssColor>,
    }
    impl VariableProvider for StaticProvider {
        fn name(&self) -> &str {
            &self.name
        }
        fn get_value(&self, _arg: Option<&str>) -> String {
            self.value.clone()
        }
        fn get_color(&self, _arg: Option<&str>) -> Option<CssColor> {
            self.color.clone()
        }
    }

    /// Provider that returns its argument so we can verify the
    /// `{name:arg}` plumbing.
    struct EchoArgProvider;
    impl VariableProvider for EchoArgProvider {
        fn name(&self) -> &str {
            "echo"
        }
        fn get_value(&self, arg: Option<&str>) -> String {
            arg.unwrap_or("").to_string()
        }
    }

    #[test]
    fn extract_variables_finds_simple_refs() {
        let v = TemplateEngine::extract_variables("a {time} b {cwd}");
        assert_eq!(
            v,
            vec![("time".to_string(), None), ("cwd".to_string(), None)]
        );
    }

    #[test]
    fn extract_variables_with_argument() {
        let v = TemplateEngine::extract_variables("{cmd:foo}");
        assert_eq!(v, vec![("cmd".to_string(), Some("foo".to_string()))]);
    }

    #[test]
    fn extract_variables_skips_malformed_braces() {
        let v = TemplateEngine::extract_variables("{ } {bad-name} {good}");
        assert_eq!(v, vec![("good".to_string(), None)]);
    }

    #[test]
    fn resolve_unknown_variable_is_empty() {
        let engine = TemplateEngine::new();
        assert_eq!(engine.resolve("a {missing} b"), "a  b");
    }

    #[test]
    fn resolve_provider_value_substituted() {
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "time".to_string(),
            value: "12:34".to_string(),
            color: None,
        }));
        assert_eq!(engine.resolve("now: {time}"), "now: 12:34");
    }

    #[test]
    fn resolve_wraps_value_in_font_when_color_present() {
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "git_branch".to_string(),
            value: "main".to_string(),
            color: Some(CssColor::Hex {
                r: 0x4c,
                g: 0xaf,
                b: 0x50,
            }),
        }));
        let r = engine.resolve("{git_branch}");
        // `r##"…"##`: the expected string contains `"#` (the `"#4caf50`),
        // which would prematurely close a single-hash raw string.
        assert_eq!(r, r##"<font color="#4caf50">main</font>"##);
    }

    #[test]
    fn resolve_passes_font_color_markup_through() {
        // A `{cmd}` that emits the status-bar color markup keeps its
        // `<font color>` so the downstream parser colors the value.
        // Mirrors the real co2.sh output.
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "cmd".to_string(),
            value: r#"CO2: <font color="limegreen">594</font>ppm"#.to_string(),
            color: None,
        }));
        let r = engine.resolve("{cmd}");
        assert_eq!(r, r#"CO2: <font color="limegreen">594</font>ppm"#);

        // End-to-end: parse + flatten yields a colored middle run.
        use crate::html;
        let runs = html::to_rich_text_runs(&html::parse(&r));
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].text, "594");
        assert_eq!(
            runs[1].color,
            Some(CssColor::Named("limegreen".to_string()))
        );
    }

    #[test]
    fn resolve_font_color_attribute_is_revalidated() {
        // A crafted color attribute must not break out of the tag: the
        // value is re-parsed and re-serialized from the parsed color.
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "cmd".to_string(),
            value: r#"<font color="red&quot;&gt;&lt;b&gt;">x</font>"#.to_string(),
            color: None,
        }));
        // The bogus color fails `parse_css_color`, so no `<font>` tag is
        // emitted; the inner text survives unstyled.
        assert_eq!(engine.resolve("{cmd}"), "x");
    }

    #[test]
    fn resolve_argument_passes_through() {
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(EchoArgProvider));
        assert_eq!(engine.resolve("{echo:hello}"), "hello");
    }

    #[test]
    fn resolve_preserves_literal_braces_for_malformed_input() {
        let engine = TemplateEngine::new();
        assert_eq!(engine.resolve("{ } {bad-"), "{ } {bad-");
    }

    #[test]
    fn resolve_handles_utf8_text() {
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "name".to_string(),
            value: "世界".to_string(),
            color: None,
        }));
        assert_eq!(engine.resolve("こんにちは {name}"), "こんにちは 世界");
    }

    #[test]
    fn register_replaces_existing_provider() {
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "x".to_string(),
            value: "old".to_string(),
            color: None,
        }));
        engine.register(Arc::new(StaticProvider {
            name: "x".to_string(),
            value: "new".to_string(),
            color: None,
        }));
        assert_eq!(engine.resolve("{x}"), "new");
    }

    /// A provider that returns non-font markup (e.g. a custom
    /// `{cmd:foo}` whose stdout prints `<b>x</b>`) MUST NOT inject
    /// styling or scripts through the downstream `html::parse` pass.
    /// `sanitize_provider_value` drops every non-`<font>` tag, so a
    /// stray `<b>x</b>` reaches the parser as the plain text `x` —
    /// neither bold nor rendered as literal `<b>x</b>`.
    #[test]
    fn provider_value_with_non_font_tags_is_neutralized() {
        use crate::html;

        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "cmd".to_string(),
            value: "<b>x</b>".to_string(),
            color: None,
        }));
        // `<b>`/`</b>` are dropped; the residue is the literal text `x`.
        let resolved = engine.resolve("{cmd}");
        assert_eq!(resolved, "x");

        // Downstream parse + flatten yields a single non-bold run.
        let nodes = html::parse(&resolved);
        let runs = html::to_rich_text_runs(&nodes);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].bold, "provider value must not become a bold run");
        assert_eq!(runs[0].text, "x");
    }

    /// Sanitization also applies inside the color-wrapping `<font>`
    /// branch (FR-Security: untrusted text must not break out of the
    /// `<font color="…">` wrapper or inject a nested tag).
    #[test]
    fn provider_value_with_html_inside_color_font_is_sanitized() {
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "git_branch".to_string(),
            value: r#"main"><script>x</script>"#.to_string(),
            color: Some(CssColor::Named("red".to_string())),
        }));
        let r = engine.resolve("{git_branch}");
        // `<script>x</script>` is dropped; the residue `main">` is
        // HTML-escaped (`"` and `>` become entities) so the wrapper
        // `<font>` attribute stays intact.
        assert_eq!(r, r#"<font color="red">main&quot;&gt;x</font>"#);
    }

    #[test]
    fn provider_inline_non_font_color_is_stripped_to_plain_text() {
        // A WebView-era helper that still emits `<span style="color:…">`
        // (the old convention) loses the color but keeps the text — the
        // span tag is not in the status-bar allowlist (`<font>` only).
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "cmd".to_string(),
            value: r#"CO2: <span style="color:tomato">1450</span>ppm"#.to_string(),
            color: None,
        }));
        assert_eq!(engine.resolve("{cmd}"), "CO2: 1450ppm");
    }

    #[test]
    fn sanitize_plain_text_has_no_tags() {
        assert_eq!(sanitize_provider_value("plain text"), "plain text");
    }

    #[test]
    fn sanitize_keeps_font_color_drops_others() {
        assert_eq!(
            sanitize_provider_value(r#"a<font color="red">b</font><b>c</b>"#),
            r#"a<font color="red">b</font>c"#
        );
    }

    #[test]
    fn sanitize_keeps_utf8_outside_tags() {
        assert_eq!(
            sanitize_provider_value("こんにちは<b>世界</b>!"),
            "こんにちは世界!"
        );
    }

    #[test]
    fn sanitize_closes_unclosed_font() {
        // A provider that opens `<font>` without closing it (malformed /
        // truncated output) must NOT leave a dangling open tag — otherwise
        // the downstream lenient parser bleeds the color to EOF.
        assert_eq!(
            sanitize_provider_value(r#"<font color="red">x"#),
            r#"<font color="red">x</font>"#
        );
    }

    #[test]
    fn sanitize_closes_multiple_unclosed_fonts_in_order() {
        // Nested unclosed fonts each get a matching close, innermost first.
        assert_eq!(
            sanitize_provider_value(r#"<font color="red">a<font color="blue">b"#),
            r#"<font color="red">a<font color="blue">b</font></font>"#
        );
    }

    #[test]
    fn sanitize_unclosed_font_does_not_bleed_into_trailing_template() {
        // End-to-end: an unclosed provider font must not color the literal
        // template text that follows the substitution.
        use crate::html;
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "cmd".to_string(),
            value: r#"<font color="red">594"#.to_string(),
            color: None,
        }));
        let resolved = engine.resolve("{cmd} ppm");
        let runs = html::to_rich_text_runs(&html::parse(&resolved));
        // The trailing " ppm" must be an uncolored run, not red.
        let trailing = runs
            .iter()
            .find(|r| r.text.contains("ppm"))
            .expect("trailing text run missing");
        assert!(
            trailing.color.is_none(),
            "color bled into trailing template text: {trailing:?}"
        );
    }
}
