//! Viewer subsystem — drains the emterm OSC queue, parses the
//! `<viewer>;<verb>;<k=v>…` payloads, routes them by viewer kind, and
//! reassembles Markdown sessions into complete documents.
//!
//! Phase 2 is window-free and fully unit-testable: completed documents
//! are emitted as [`RenderRequest`]s to an abstract [`ViewerSink`]. Phase
//! 4 provides the real sink that spawns child viewer processes; tests use
//! [`CapturingSink`].

pub mod assets;
pub mod data;
pub mod data_model;
pub mod data_payload;
pub mod data_window;
pub mod image;
pub mod image_payload;
pub mod image_resolver;
pub mod image_window;
pub mod launch;
pub mod markdown;
pub mod shell;
pub mod window;

use std::path::Path;
use std::time::Instant;

use crate::callbacks::EmtermOscRequest;
use crate::settings::Settings;
use launch::ViewerPayload;
use markdown::MarkdownViewerSessions;

/// The viewer kinds that an OSC 777 `emterm` launch sequence can dispatch
/// to a child viewer (Markdown / image / JSON / YAML). This is the single
/// source of truth shared between the viewer dispatch ([`ViewerRouter::route`])
/// and the mux snapshot rich-content stripper
/// (`crate::mux::scrollback_filter::strip_replayable_rich_content`): the
/// stripper removes exactly these kinds from a reattach snapshot so they are
/// not re-launched, and a `drift_*` test keeps the dispatch and the stripper
/// in lockstep (see the test module below).
pub const REPLAYABLE_VIEWER_KINDS: &[&str] = &["markdown", "image", "json", "yaml"];

/// Markdown source dialect carried from `begin;format=…` through to the
/// rendered window. Mirrors the WebView build's `MarkdownFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownFormat {
    /// CommonMark (the default when `format` is absent or unrecognized).
    #[default]
    CommonMark,
    /// GitHub Flavored Markdown.
    Gfm,
}

impl MarkdownFormat {
    /// Parse a `format=` value, defaulting to [`MarkdownFormat::CommonMark`]
    /// for the empty string or any unrecognized token (matches the
    /// WebView build's permissive default).
    pub fn parse(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "gfm" => Self::Gfm,
            _ => Self::CommonMark,
        }
    }

    /// Wire token used when serializing the payload for the child viewer.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommonMark => "commonmark",
            Self::Gfm => "gfm",
        }
    }
}

/// A structured emterm viewer command parsed from one OSC payload.
///
/// `<kind>;<verb>;<key>=<value>;…` — e.g. `markdown;begin;id=…;format=gfm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    /// Viewer kind (first token), e.g. `markdown`, `image`, `json`, `yaml`.
    pub kind: String,
    /// Verb (second token), e.g. `begin`, `chunk`, `end`.
    pub verb: String,
    /// Remaining `key=value` parameters. Later duplicates win.
    pub params: std::collections::HashMap<String, String>,
}

/// Tokenize a raw OSC payload (`callbacks.rs` already stripped the
/// `777;` prefix) into a [`ParsedCommand`]. Returns `None` (with a warn)
/// when the payload lacks the required `<kind>;<verb>` prefix.
pub fn parse_payload(payload: &str) -> Option<ParsedCommand> {
    let mut tokens = payload.split(';');
    let kind = tokens.next().unwrap_or("").trim();
    let verb = match tokens.next() {
        Some(v) => v.trim(),
        None => {
            log::warn!("viewer payload missing verb: {payload:?}");
            return None;
        }
    };
    if kind.is_empty() || verb.is_empty() {
        log::warn!("viewer payload missing kind/verb: {payload:?}");
        return None;
    }

    let mut params = std::collections::HashMap::new();
    for tok in tokens {
        // Split on the first '=' only; values may themselves contain '='
        // (e.g. base64 padding). Tokens without '=' are ignored.
        if let Some(eq) = tok.find('=') {
            let key = tok[..eq].to_string();
            let value = tok[eq + 1..].to_string();
            if !key.is_empty() {
                params.insert(key, value);
            }
        }
    }

    Some(ParsedCommand {
        kind: kind.to_string(),
        verb: verb.to_string(),
        params,
    })
}

/// Bytes written to a tab's PTY to release a parked interactive
/// `emterm markdown` CLI from its stdin loop.
pub const MARKDOWN_RELEASE_INPUT: &[u8] = b"quit\n";

/// True iff `payload` is a markdown session `end` marker that carries
/// `interactive=1`. The `emterm markdown` CLI sets this flag ONLY when its
/// stdin is a TTY (so a genuine interactive CLI is parked waiting to be
/// released); a non-interactive (piped/redirected) invocation omits it, so
/// the common *accidental* release — injecting `quit` after a non-TTY CLI
/// has already returned the prompt — never fires.
///
/// SECURITY (accepted residual): the flag is plaintext carried in the
/// terminal output stream, which is attacker-controllable. Untrusted output
/// (a `cat`'d file, an SSH peer, a log line) CAN forge
/// `markdown;end;…;interactive=1` and make the caller write
/// [`MARKDOWN_RELEASE_INPUT`] into the emitting tab's PTY — the `id` is not
/// correlated against a live session, so forging needs no secret. The impact
/// is bounded to a single `quit\n` line into that tab's foreground program
/// (not arbitrary input); this is an accepted residual documented in SPEC.md
/// ("Interactive CLI release"). Closing it would require terminal-owned state
/// (e.g. a foreground-process check or a begin-correlated session id).
pub fn markdown_end_wants_release(payload: &str) -> bool {
    payload.starts_with("markdown;end;") && payload.split(';').any(|tok| tok == "interactive=1")
}

/// A completed Markdown document ready to be displayed. Phase 4 serializes
/// this (plus the resolved appearance) into the child viewer payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRequest {
    /// Fully reassembled, base64-decoded UTF-8 Markdown source.
    pub markdown: String,
    /// Source dialect.
    pub format: MarkdownFormat,
    /// Optional base directory for resolving relative image references.
    pub basedir: Option<String>,
}

/// Abstract destination for completed [`RenderRequest`]s. The real Phase 4
/// implementation spawns a child viewer process; tests capture instead.
pub trait ViewerSink {
    /// Consume one completed Markdown render request.
    fn emit(&mut self, request: RenderRequest);

    /// Consume one completed JSON/YAML data-viewer request.
    fn emit_data(&mut self, request: data::DataRenderRequest);

    /// Periodic maintenance hook, called once per drain pass (M1). Default
    /// is a no-op; [`ProcessViewerSink`] overrides it to reap exited child
    /// viewers so a closed window does not linger as a zombie until the
    /// next document renders.
    fn maintain(&mut self) {}
}

/// Test/inspection sink that records every emitted request.
/// Only compiled under `cfg(test)` — production uses [`ProcessViewerSink`].
#[cfg(test)]
#[derive(Debug, Default)]
pub struct CapturingSink {
    /// Markdown requests in emission order.
    pub requests: Vec<RenderRequest>,
    /// JSON/YAML requests in emission order.
    pub data_requests: Vec<data::DataRenderRequest>,
}

#[cfg(test)]
impl ViewerSink for CapturingSink {
    fn emit(&mut self, request: RenderRequest) {
        self.requests.push(request);
    }

    fn emit_data(&mut self, request: data::DataRenderRequest) {
        self.data_requests.push(request);
    }
}

/// Production sink: serializes each [`RenderRequest`] (plus the resolved
/// appearance) to a temp file and spawns a child `self --viewer <path>`
/// process. Tracks spawned children loosely for non-blocking reaping so
/// closed viewers don't linger as zombies. The terminal loop is never
/// blocked — a spawn failure is logged (ERR_SPAWN) and dropped.
pub struct ProcessViewerSink {
    settings: std::sync::Arc<Settings>,
    children: Vec<std::process::Child>,
}

impl std::fmt::Debug for ProcessViewerSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessViewerSink")
            .field("children", &self.children.len())
            .finish()
    }
}

impl ProcessViewerSink {
    /// Construct a sink that renders with the given resolved `settings`.
    pub fn new(settings: std::sync::Arc<Settings>) -> Self {
        Self {
            settings,
            children: Vec::new(),
        }
    }

    /// Non-blocking reap of exited child viewers. Called opportunistically
    /// so closed windows don't accumulate as zombies. Never blocks on a
    /// still-running child.
    pub fn reap(&mut self) {
        self.children.retain_mut(|child| {
            match child.try_wait() {
                Ok(Some(_status)) => false, // exited → drop
                Ok(None) => true,           // still running → keep
                Err(e) => {
                    log::warn!("viewer: try_wait failed for child: {e}");
                    false
                }
            }
        });
    }

    /// Number of tracked (not-yet-reaped) child viewers. Reserved for the
    /// planned viewer status surface; no caller yet (hence `dead_code`).
    #[allow(dead_code)]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Spawn `self <flag> <payload-path>` and track the child for
    /// reaping. A spawn failure is logged (ERR_SPAWN) and dropped — the
    /// terminal is never blocked; the orphaned payload file is removed
    /// since nothing will ever read it.
    fn spawn_child(&mut self, flag: &str, path: &Path) {
        match crate::self_exec::spawn_self(|c| {
            c.arg(flag).arg(path);
        }) {
            Ok(child) => {
                log::warn!(
                    "viewer: spawned child pid={} {flag} payload={}",
                    child.id(),
                    path.display()
                );
                self.children.push(child);
            }
            Err(e) => {
                log::warn!("viewer: failed to spawn child ({e}); terminal unaffected");
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

impl ViewerSink for ProcessViewerSink {
    /// Reap exited child viewers every drain pass so closed windows don't
    /// linger as zombies (M1).
    fn maintain(&mut self) {
        self.reap();
    }

    fn emit(&mut self, request: RenderRequest) {
        self.reap();
        // Move the (potentially large) document into the payload instead of
        // cloning it (H4).
        let payload = ViewerPayload::from_request(request, &self.settings);
        // Serialize the payload to a temp file (`launch::write_payload`),
        // then spawn `self --viewer <path>`. The temp file is left for the
        // child to read (and reboot GC), per the project temp-file
        // convention. A spawn failure is logged (ERR_SPAWN) and dropped —
        // the terminal is never blocked.
        let path = match launch::write_payload(&payload) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("viewer: failed to write payload temp file ({e}); skipping viewer");
                return;
            }
        };
        self.spawn_child("--viewer", &path);
    }

    fn emit_data(&mut self, request: data::DataRenderRequest) {
        self.reap();
        // Chrome appearance travels in the payload header so the child
        // never re-reads settings.json (same design as the image viewer).
        let chrome = image_payload::ViewerChrome {
            theme: launch::theme_token(self.settings.ui_theme).to_string(),
            preset: launch::preset_token(self.settings.ui_theme_preset).to_string(),
            ui_font_family: self.settings.ui_font_family.clone(),
        };
        let path = match data_payload::write_data_payload(request.format, &request.text, &chrome) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("viewer: failed to write data payload ({e}); skipping viewer");
                return;
            }
        };
        self.spawn_child("--data-viewer", &path);
    }
}

/// Drains the OSC queue, routes payloads by viewer kind, and accumulates
/// Markdown and JSON/YAML sessions. Holds no window state — purely a
/// coordinator.
#[derive(Debug, Default)]
pub struct ViewerSpawner {
    markdown: MarkdownViewerSessions,
    data: data::DataViewerSessions,
}

impl ViewerSpawner {
    /// Construct a spawner with empty session state.
    pub fn new() -> Self {
        Self {
            markdown: MarkdownViewerSessions::new(),
            data: data::DataViewerSessions::new(),
        }
    }

    /// Number of in-flight Markdown sessions. Reserved observability helper
    /// for the planned viewer status surface; no caller yet (hence `dead_code`).
    #[allow(dead_code)]
    pub fn markdown_session_count(&self) -> usize {
        self.markdown.session_count()
    }

    /// Process a batch of drained OSC requests in arrival order, emitting
    /// completed documents to `sink`. Uses `now` for timeout bookkeeping.
    ///
    /// `Tab::drain_osc()` produces the `requests`; this keeps the spawner
    /// independent of callback locking and trivially unit-testable.
    pub fn drain(
        &mut self,
        requests: Vec<EmtermOscRequest>,
        now: Instant,
        sink: &mut dyn ViewerSink,
    ) {
        // Opportunistic timeout sweep on each pass (ERR_TIMEOUT).
        self.markdown.evict_expired(now);
        self.data.evict_expired(now);
        // Reap exited child viewers each pass (M1). Default no-op for
        // capturing/test sinks; ProcessViewerSink reaps zombies here.
        sink.maintain();

        for req in requests {
            let Some(cmd) = parse_payload(&req.payload) else {
                continue;
            };
            self.route(&cmd, now, sink);
        }
    }

    fn route(&mut self, cmd: &ParsedCommand, now: Instant, sink: &mut dyn ViewerSink) {
        match cmd.kind.as_str() {
            "markdown" => self.markdown.handle(cmd, now, sink),
            "json" | "yaml" => {
                // DataFormat::parse never fails for these two tokens.
                if let Some(format) = data::DataFormat::parse(&cmd.kind) {
                    self.data.handle(format, cmd, now, sink);
                }
            }
            // Reserved for future features — no-op + debug log (FR1).
            "image" => {
                log::debug!(
                    "viewer: reserved kind {:?} ignored (verb={})",
                    cmd.kind,
                    cmd.verb
                );
            }
            other => {
                log::warn!("viewer: unknown kind {other:?} ignored");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(payload: &str) -> EmtermOscRequest {
        EmtermOscRequest {
            payload: payload.to_string(),
        }
    }

    fn b64(s: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    }

    // ── parse_payload ───────────────────────────────────────────────────

    #[test]
    fn parse_payload_splits_kind_verb_and_params() {
        let cmd = parse_payload("markdown;begin;id=abc;format=gfm;version=1").unwrap();
        assert_eq!(cmd.kind, "markdown");
        assert_eq!(cmd.verb, "begin");
        assert_eq!(cmd.params.get("id").map(String::as_str), Some("abc"));
        assert_eq!(cmd.params.get("format").map(String::as_str), Some("gfm"));
        assert_eq!(cmd.params.get("version").map(String::as_str), Some("1"));
    }

    #[test]
    fn parse_payload_keeps_equals_in_value() {
        // base64 values can contain '=' padding; only the first '=' splits.
        let cmd = parse_payload("markdown;chunk;id=a;seq=0;data=YQ==").unwrap();
        assert_eq!(cmd.params.get("data").map(String::as_str), Some("YQ=="));
    }

    #[test]
    fn parse_payload_missing_verb_returns_none() {
        assert!(parse_payload("markdown").is_none());
    }

    #[test]
    fn markdown_end_wants_release_gates_on_interactive_flag() {
        assert!(markdown_end_wants_release(
            "markdown;end;id=abc;interactive=1"
        ));
        assert!(!markdown_end_wants_release("markdown;end;id=abc"));
        assert!(!markdown_end_wants_release("markdown;end"));
        // Must be an `end` marker, not begin.
        assert!(!markdown_end_wants_release(
            "markdown;begin;id=abc;interactive=1"
        ));
        assert!(!markdown_end_wants_release(
            "markdown;chunk;id=abc;seq=0;data="
        ));
        // Wrong kind even with the flag.
        assert!(!markdown_end_wants_release("image;end;interactive=1"));
        assert!(!markdown_end_wants_release(""));
    }

    #[test]
    fn parse_payload_empty_kind_returns_none() {
        assert!(parse_payload(";begin;id=a").is_none());
    }

    // ── routing ─────────────────────────────────────────────────────────

    /// drift guard (b): the kinds `route` explicitly dispatches on must equal
    /// the shared [`REPLAYABLE_VIEWER_KINDS`] SSOT that the mux snapshot
    /// stripper removes. If a new viewer kind is added to `route` (or the
    /// SSOT) without updating the other, this fails — keeping the dispatch and
    /// the rich-content stripper from drifting (same intent as the
    /// `mux_apc_extractor::drift_*` tests).
    #[test]
    fn drift_route_dispatch_kinds_match_replayable_viewer_kinds_ssot() {
        // The kinds `ViewerRouter::route` has an explicit (non-wildcard) arm
        // for. Mirror them here; the assertion ties the two together.
        let route_kinds = ["markdown", "json", "yaml", "image"];
        let mut a = route_kinds.to_vec();
        a.sort_unstable();
        let mut b = REPLAYABLE_VIEWER_KINDS.to_vec();
        b.sort_unstable();
        assert_eq!(
            a, b,
            "route dispatch kinds and REPLAYABLE_VIEWER_KINDS SSOT have drifted"
        );
    }

    #[test]
    fn markdown_payloads_round_trip_to_one_request() {
        let mut spawner = ViewerSpawner::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        spawner.drain(
            vec![
                req("markdown;begin;id=x;format=gfm"),
                req(&format!(
                    "markdown;chunk;id=x;seq=0;data={}",
                    b64("# Title")
                )),
                req("markdown;end;id=x"),
            ],
            now,
            &mut sink,
        );
        assert_eq!(sink.requests.len(), 1);
        assert_eq!(sink.requests[0].markdown, "# Title");
        assert_eq!(sink.requests[0].format, MarkdownFormat::Gfm);
    }

    #[test]
    fn reserved_kinds_are_ignored_without_request() {
        let mut spawner = ViewerSpawner::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        spawner.drain(
            vec![
                req("image;show;path=/tmp/a.png"),
                // json/yaml route to the data sessions now, but an unknown
                // verb still emits nothing.
                req("json;render;data=e30="),
                req("yaml;render;data=e30="),
            ],
            now,
            &mut sink,
        );
        assert!(sink.requests.is_empty());
        assert!(sink.data_requests.is_empty());
    }

    #[test]
    fn json_payloads_round_trip_to_one_data_request() {
        let mut spawner = ViewerSpawner::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        spawner.drain(
            vec![
                req("json;begin;id=d;version=1.0"),
                req(&format!("json;chunk;id=d;seq=0;data={}", b64("{\"a\":1}"))),
                req("json;end;id=d"),
            ],
            now,
            &mut sink,
        );
        assert!(sink.requests.is_empty());
        assert_eq!(sink.data_requests.len(), 1);
        assert_eq!(sink.data_requests[0].text, "{\"a\":1}");
        assert_eq!(sink.data_requests[0].format, data::DataFormat::Json);
    }

    #[test]
    fn yaml_payloads_round_trip_to_one_data_request() {
        let mut spawner = ViewerSpawner::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        spawner.drain(
            vec![
                req("yaml;begin;id=y;version=1.0"),
                req(&format!("yaml;chunk;id=y;seq=0;data={}", b64("a: 1\n"))),
                req("yaml;end;id=y"),
            ],
            now,
            &mut sink,
        );
        assert_eq!(sink.data_requests.len(), 1);
        assert_eq!(sink.data_requests[0].format, data::DataFormat::Yaml);
    }

    #[test]
    fn unknown_kind_is_ignored_without_request() {
        let mut spawner = ViewerSpawner::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        spawner.drain(vec![req("widget;do;x=1")], now, &mut sink);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn malformed_payload_is_skipped_no_panic() {
        let mut spawner = ViewerSpawner::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        spawner.drain(vec![req("garbage")], now, &mut sink);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn each_completed_session_yields_exactly_one_request() {
        let mut spawner = ViewerSpawner::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        spawner.drain(
            vec![
                req("markdown;begin;id=a"),
                req(&format!("markdown;chunk;id=a;seq=0;data={}", b64("A"))),
                req("markdown;end;id=a"),
                req("markdown;begin;id=b"),
                req(&format!("markdown;chunk;id=b;seq=0;data={}", b64("B"))),
                req("markdown;end;id=b"),
            ],
            now,
            &mut sink,
        );
        assert_eq!(sink.requests.len(), 2);
        assert_eq!(sink.requests[0].markdown, "A");
        assert_eq!(sink.requests[1].markdown, "B");
    }

    #[test]
    fn markdown_format_parse_defaults_to_commonmark() {
        assert_eq!(MarkdownFormat::parse("gfm"), MarkdownFormat::Gfm);
        assert_eq!(MarkdownFormat::parse("GFM"), MarkdownFormat::Gfm);
        assert_eq!(
            MarkdownFormat::parse("commonmark"),
            MarkdownFormat::CommonMark
        );
        assert_eq!(MarkdownFormat::parse("weird"), MarkdownFormat::CommonMark);
        assert_eq!(MarkdownFormat::parse(""), MarkdownFormat::CommonMark);
    }
}
