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

use crate::html::CssColor;

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

/// Push the substituted text into `out`, wrapping in
/// `<span style="color:#rrggbb">…</span>` when a color is supplied.
/// The text is HTML-escaped before being embedded so a provider that
/// returns markup (e.g. a `{cmd:foo}` that prints `<b>x</b>` or
/// `<script>...`) cannot inject styling or scripts through the
/// downstream `html::parse` pass.
///
/// Provider output is also passed through [`strip_html_tags_naive`]
/// first: many WebView-era `{cmd:…}` helper scripts emit inline
/// `<span style="color:…">value</span>` for coloring (CO2, weather,
/// etc.). Until the upstream HTML parser is plumbed through this path,
/// the strip keeps the surrounding plain text readable instead of
/// rendering the raw markup. The stripper is intentionally crude (no
/// quoted-attribute awareness) and will be replaced once the real
/// parser lands.
fn push_styled(out: &mut String, value: &str, color: Option<&CssColor>) {
    let stripped = strip_html_tags_naive(value);
    let escaped = escape_html(stripped.as_ref());
    if let Some(color) = color {
        let css = css_color_string(color);
        out.push_str(&format!(r#"<span style="color:{css}">{escaped}</span>"#));
    } else {
        out.push_str(&escaped);
    }
}

/// Strip every `<…>` block from `input`. Naive: no awareness of quoted
/// attributes, comments, or CDATA. Borrowed return path when the input
/// contains no `<` byte. Temporary; see [`push_styled`].
fn strip_html_tags_naive(input: &str) -> Cow<'_, str> {
    if !input.contains('<') {
        return Cow::Borrowed(input);
    }
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' {
            // Skip up to the next `>` (or EOF).
            let mut j = i + 1;
            while j < len && bytes[j] != b'>' {
                j += 1;
            }
            if j >= len {
                // Unterminated `<…`: preserve verbatim so the user can
                // see the broken markup rather than silently dropping
                // arbitrary trailing content.
                out.push_str(&input[i..]);
                break;
            }
            i = j + 1;
        } else {
            let cp_len = utf8_len(bytes[i]).max(1);
            let end = (i + cp_len).min(len);
            out.push_str(&input[i..end]);
            i = end;
        }
    }
    Cow::Owned(out)
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
    fn resolve_wraps_value_in_span_when_color_present() {
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
        assert_eq!(r, r#"<span style="color:#4caf50">main</span>"#);
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

    /// A provider that returns markup-looking text (e.g. a custom
    /// `{cmd:foo}` whose stdout happens to print `<b>x</b>`) MUST NOT
    /// be able to inject styling or scripts through the downstream
    /// `html::parse` pass. Until the real HTML parser is wired into
    /// the status-bar path, [`push_styled`] strips `<…>` blocks before
    /// HTML-escaping the residue, so a stray `<b>x</b>` reaches the
    /// downstream parser as the plain text `x` — neither styled nor
    /// rendered as literal `<b>x</b>`.
    #[test]
    fn provider_value_containing_html_tags_is_stripped() {
        use crate::html;

        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "cmd".to_string(),
            value: "<b>x</b>".to_string(),
            color: None,
        }));
        // The tag-stripper removes `<b>` and `</b>`; the residue is
        // HTML-escaped (no entities to escape here) and appears as
        // the literal text `x`.
        let resolved = engine.resolve("{cmd}");
        assert_eq!(resolved, "x");

        // Downstream parse + flatten yields a single non-bold run.
        let nodes = html::parse(&resolved);
        let runs = html::to_rich_text_runs(&nodes);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].bold, "provider value must not become a bold run");
        assert_eq!(runs[0].text, "x");
    }

    /// The strip-and-escape also applies inside the color-wrapping
    /// span branch (FR-Security: untrusted text must not break out of
    /// the `<span style="…">` wrapper or inject a nested tag).
    #[test]
    fn provider_value_with_html_inside_color_span_is_stripped() {
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "git_branch".to_string(),
            value: r#"main"><script>x</script>"#.to_string(),
            color: Some(CssColor::Named("red".to_string())),
        }));
        let r = engine.resolve("{git_branch}");
        // `<script>x</script>` is removed by the tag-stripper. The
        // residue `main">` is HTML-escaped (`"` and `>` become
        // entities) so the wrapper span attribute stays intact.
        assert_eq!(r, r#"<span style="color:red">main&quot;&gt;x</span>"#);
    }

    // ── temporary HTML-tag stripper (CO2 / weather helper scripts) ─────

    #[test]
    fn provider_inline_span_color_is_stripped_to_plain_text() {
        // Reproduces the user's CO2 helper which emits e.g.
        // `CO2: <span style="color:tomato">1450</span>ppm` to colorize
        // the value in the WebView build. Until the egui renderer
        // wires through the real HTML parser, the strip step keeps the
        // surrounding text readable.
        let mut engine = TemplateEngine::new();
        engine.register(Arc::new(StaticProvider {
            name: "cmd".to_string(),
            value: r#"CO2: <span style="color:tomato">1450</span>ppm"#.to_string(),
            color: None,
        }));
        assert_eq!(engine.resolve("{cmd}"), "CO2: 1450ppm");
    }

    #[test]
    fn strip_html_tags_naive_no_tags_is_borrowed() {
        // Borrowed return path keeps the no-markup hot path allocation-free.
        let s = "plain text";
        let out = strip_html_tags_naive(s);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, "plain text");
    }

    #[test]
    fn strip_html_tags_naive_removes_simple_tags() {
        assert_eq!(strip_html_tags_naive("a<b>x</b>c").as_ref(), "axc");
        assert_eq!(strip_html_tags_naive("<br/>line").as_ref(), "line");
        assert_eq!(strip_html_tags_naive("<p>1</p><p>2</p>").as_ref(), "12");
    }

    #[test]
    fn strip_html_tags_naive_unterminated_tag_preserved() {
        // An unterminated `<…` is preserved verbatim so the user can
        // see the broken markup (this matches the doc comment on
        // `strip_html_tags_naive`).
        assert_eq!(strip_html_tags_naive("ok <unterm").as_ref(), "ok <unterm");
    }

    #[test]
    fn strip_html_tags_naive_keeps_utf8_outside_tags() {
        assert_eq!(
            strip_html_tags_naive("こんにちは<b>世界</b>!").as_ref(),
            "こんにちは世界!"
        );
    }

    #[test]
    fn strip_html_tags_naive_strips_attributes_with_spaces() {
        assert_eq!(
            strip_html_tags_naive(r#"<span style="color: red">x</span>"#).as_ref(),
            "x"
        );
    }
}
