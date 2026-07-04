//! `HtmlViewerSessions` — the `html` OSC 777 session accumulator, plus the
//! HTML viewer's payload struct and temp-file writer.
//!
//! Mirrors the Markdown session lifecycle (`markdown.rs`, Decision D4 in
//! IMPLEMENTATION.md) as a SEPARATE module rather than generalizing the
//! Markdown one: `begin` captures an optional `basedir`, `chunk` stores raw
//! base64 fragments keyed by `seq`, and `end` joins them in `seq` order,
//! decodes once, and emits an [`HtmlRenderRequest`] to the caller-supplied
//! [`ViewerSink`]. No window is created here — task0004 provides the real
//! child process; tests use a capturing sink.
//!
//! The payload hand-off (JSON temp file, `create_new` + 0o600 on Unix)
//! follows the shared-component contract in IMPLEMENTATION.md: only the
//! HTML text and optional basedir travel in the file — no appearance data,
//! since the raw document renders with its own styles only.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use base64::Engine;

use super::image_payload::open_create_new;
use super::markdown::{MAX_SESSION_DATA_SIZE, MAX_SESSIONS, SESSION_TIMEOUT};
use super::{ParsedCommand, ViewerSink};

/// Child flag passed to `self` when spawning the HTML viewer window
/// (Decision D1 in IMPLEMENTATION.md). Dispatched in `main.rs` by
/// task0004; this module's [`super::ProcessViewerSink::emit_html`] is the
/// sole producer of the spawn argument.
pub const HTML_VIEWER_FLAG: &str = "--html-viewer";

/// A completed HTML document ready to hand off to the child viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlRenderRequest {
    /// Fully reassembled, base64-decoded UTF-8 HTML source.
    pub html: String,
    /// Optional base directory for resolving relative document references
    /// (task0003 owns the resolver; this module only carries the value).
    pub basedir: Option<String>,
}

/// One in-flight `html` session keyed by its `id`.
#[derive(Debug)]
struct Session {
    basedir: Option<String>,
    /// Raw base64 chunk text indexed by `seq`. Stored *undecoded*, same
    /// rationale as the Markdown/data sessions: the base64 stream is
    /// concatenated in `seq` order and decoded exactly once on `end`, so
    /// chunks split mid-quantum or across a UTF-8 char boundary reassemble
    /// correctly.
    chunks: HashMap<u64, String>,
    /// Cumulative size of the *encoded* base64 text accepted so far.
    encoded_size: usize,
    last_activity: Instant,
}

/// Manages the begin/chunk/end lifecycle for `html` viewer sessions.
#[derive(Debug, Default)]
pub struct HtmlViewerSessions {
    sessions: HashMap<String, Session>,
}

impl HtmlViewerSessions {
    /// Construct an empty session manager.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Number of in-flight sessions (test/observability helper).
    #[allow(dead_code)]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Handle one parsed `html` command, using `now` as the clock for
    /// timeout bookkeeping. On a successful `end`, an [`HtmlRenderRequest`]
    /// is pushed to `sink`.
    pub fn handle(&mut self, cmd: &ParsedCommand, now: Instant, sink: &mut dyn ViewerSink) {
        match cmd.verb.as_str() {
            "begin" => self.handle_begin(cmd, now),
            "chunk" => self.handle_chunk(cmd, now),
            "end" => self.handle_end(cmd, sink),
            other => {
                log::warn!("html viewer: unknown verb {other:?}");
            }
        }
    }

    /// Drop sessions idle for longer than [`SESSION_TIMEOUT`]. Called
    /// opportunistically on each drain pass by the spawner.
    pub fn evict_expired(&mut self, now: Instant) {
        let before = self.sessions.len();
        self.sessions
            .retain(|_, s| now.duration_since(s.last_activity) <= SESSION_TIMEOUT);
        let dropped = before - self.sessions.len();
        if dropped > 0 {
            log::warn!("html viewer: dropped {dropped} timed-out session(s)");
        }
    }

    fn handle_begin(&mut self, cmd: &ParsedCommand, now: Instant) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("html begin: missing id");
            return;
        };
        if self.sessions.len() >= MAX_SESSIONS {
            log::warn!("html begin: max sessions ({MAX_SESSIONS}) reached, rejecting {id}");
            return;
        }
        let basedir = cmd.params.get("basedir").cloned().filter(|s| !s.is_empty());

        self.sessions.insert(
            id.clone(),
            Session {
                basedir,
                chunks: HashMap::new(),
                encoded_size: 0,
                last_activity: now,
            },
        );
    }

    fn handle_chunk(&mut self, cmd: &ParsedCommand, now: Instant) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("html chunk: missing id");
            return;
        };
        let Some(session) = self.sessions.get_mut(id) else {
            log::warn!("html chunk: unknown session {id}");
            return;
        };
        let Some(seq_str) = cmd.params.get("seq") else {
            log::warn!("html chunk: missing seq for {id}");
            return;
        };
        let Ok(seq) = seq_str.parse::<u64>() else {
            log::warn!("html chunk: invalid seq {seq_str:?} for {id}");
            return;
        };
        let Some(data) = cmd.params.get("data") else {
            log::warn!("html chunk: missing data for {id}");
            return;
        };

        if session.encoded_size.saturating_add(data.len()) > MAX_SESSION_DATA_SIZE {
            log::warn!("html chunk: size cap exceeded for {id}, dropping session");
            self.sessions.remove(id);
            return;
        }

        session.encoded_size += data.len();
        session.chunks.insert(seq, data.clone());
        session.last_activity = now;
    }

    fn handle_end(&mut self, cmd: &ParsedCommand, sink: &mut dyn ViewerSink) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("html end: missing id");
            return;
        };
        let Some(session) = self.sessions.remove(id) else {
            log::warn!("html end: unknown session {id}");
            return;
        };

        let mut pairs: Vec<(u64, String)> = session.chunks.into_iter().collect();
        pairs.sort_unstable_by_key(|(seq, _)| *seq);
        let mut encoded = String::with_capacity(session.encoded_size);
        for (_, b64) in pairs {
            encoded.push_str(&b64);
        }

        let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded.as_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                log::warn!("html end: invalid base64 for {id}, dropping session");
                return;
            }
        };
        let html = match String::from_utf8(decoded) {
            Ok(s) => s,
            Err(_) => {
                log::warn!("html end: invalid UTF-8 for {id}, dropping session");
                return;
            }
        };

        sink.emit_html(HtmlRenderRequest {
            html,
            basedir: session.basedir,
        });
    }
}

/// Serializable payload written to the temp file and read by the child
/// (task0004). No appearance fields — the raw document renders with its
/// own styles only (IMPLEMENTATION.md shared-component contract).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HtmlPayload {
    /// Reassembled UTF-8 HTML source.
    pub html: String,
    /// Optional base directory for relative document references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basedir: Option<String>,
}

impl HtmlPayload {
    /// Build from a completed [`HtmlRenderRequest`] (by value, avoiding a
    /// clone of a potentially large document).
    pub fn from_request(request: HtmlRenderRequest) -> Self {
        Self {
            html: request.html,
            basedir: request.basedir,
        }
    }

    /// Serialize to a JSON string for the temp file.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from the JSON read out of the temp file.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Serialize `payload` to a uniquely named temp file under the OS temp dir
/// and return its path. The file is created with `create_new(true)` (no
/// clobber) and, on Unix, mode 0o600 (owner-read/write only) — same
/// discipline as the Markdown/image/data viewer payloads. Not removed
/// here — the child (task0004) reads it on startup and deletes it; `/tmp`
/// is cleared on reboot as a fallback (project temp-file convention).
pub fn write_payload(payload: &HtmlPayload) -> std::io::Result<PathBuf> {
    use std::io::Write as _;
    let json = payload
        .to_json()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let path = temp_payload_path();
    let mut f = open_create_new(&path)?;
    f.write_all(json.as_bytes())?;
    Ok(path)
}

/// Build a unique payload temp-file path under the OS temp dir. Same
/// scheme as the sibling viewer payloads (PID + wall-clock nanos +
/// monotonic counter).
fn temp_payload_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("emterm-html-viewer-{pid}-{nanos}-{n}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::CapturingSink;

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    }

    fn cmd(verb: &str, pairs: &[(&str, &str)]) -> ParsedCommand {
        ParsedCommand {
            kind: "html".to_string(),
            verb: verb.to_string(),
            params: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    // ── AC-2: begin/chunk/end round trip, basedir, out-of-order chunks ──

    #[test]
    fn begin_chunk_end_joins_in_seq_order_and_decodes() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(
            &cmd(
                "chunk",
                &[("id", "a"), ("seq", "0"), ("data", &b64("<p>Hello "))],
            ),
            t,
            &mut sink,
        );
        mgr.handle(
            &cmd(
                "chunk",
                &[("id", "a"), ("seq", "1"), ("data", &b64("World</p>"))],
            ),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);

        assert_eq!(sink.html_requests.len(), 1);
        assert_eq!(sink.html_requests[0].html, "<p>Hello World</p>");
        assert_eq!(mgr.session_count(), 0);
    }

    #[test]
    fn basedir_is_carried_into_request() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(
            &cmd("begin", &[("id", "a"), ("basedir", "/home/me/docs")]),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(
            sink.html_requests[0].basedir.as_deref(),
            Some("/home/me/docs")
        );
    }

    #[test]
    fn missing_basedir_is_none() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(sink.html_requests[0].basedir, None);
    }

    #[test]
    fn out_of_order_chunks_are_reordered_by_seq() {
        // Encode the WHOLE document, then split the base64 across chunks
        // and feed them out of `seq` order to prove reassembly sorts by seq
        // before decoding once.
        let encoded = b64("<div>ABC</div>");
        let third = encoded.len() / 3;
        let (c0, rest) = encoded.split_at(third);
        let (c1, c2) = rest.split_at(third);

        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(
            &cmd("chunk", &[("id", "a"), ("seq", "2"), ("data", c2)]),
            t,
            &mut sink,
        );
        mgr.handle(
            &cmd("chunk", &[("id", "a"), ("seq", "0"), ("data", c0)]),
            t,
            &mut sink,
        );
        mgr.handle(
            &cmd("chunk", &[("id", "a"), ("seq", "1"), ("data", c1)]),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(sink.html_requests[0].html, "<div>ABC</div>");
    }

    #[test]
    fn empty_session_emits_empty_document() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(sink.html_requests.len(), 1);
        assert_eq!(sink.html_requests[0].html, "");
    }

    #[test]
    fn interleaved_sessions_stay_independent() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(&cmd("begin", &[("id", "b")]), t, &mut sink);
        mgr.handle(
            &cmd("chunk", &[("id", "a"), ("seq", "0"), ("data", &b64("AA"))]),
            t,
            &mut sink,
        );
        mgr.handle(
            &cmd("chunk", &[("id", "b"), ("seq", "0"), ("data", &b64("BB"))]),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[("id", "b")]), t, &mut sink);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(sink.html_requests.len(), 2);
        assert_eq!(sink.html_requests[0].html, "BB");
        assert_eq!(sink.html_requests[1].html, "AA");
    }

    // ── AC-3: malformed input dropped without panic, no emission ────────

    #[test]
    fn missing_id_is_ignored_no_panic() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[]), t, &mut sink);
        mgr.handle(
            &cmd("chunk", &[("seq", "0"), ("data", &b64("x"))]),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[]), t, &mut sink);
        assert_eq!(mgr.session_count(), 0);
        assert!(sink.html_requests.is_empty());
    }

    #[test]
    fn chunk_without_begin_is_ignored_no_panic() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(
            &cmd(
                "chunk",
                &[("id", "ghost"), ("seq", "0"), ("data", &b64("x"))],
            ),
            t,
            &mut sink,
        );
        assert_eq!(mgr.session_count(), 0);
        assert!(sink.html_requests.is_empty());
    }

    #[test]
    fn end_without_begin_is_ignored_no_panic() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("end", &[("id", "ghost")]), t, &mut sink);
        assert!(sink.html_requests.is_empty());
    }

    #[test]
    fn unknown_verb_is_ignored_no_panic() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("frobnicate", &[("id", "a")]), t, &mut sink);
        assert!(sink.html_requests.is_empty());
    }

    #[test]
    fn malformed_base64_drops_session_no_panic() {
        // Decode is deferred to `end`, so a malformed-base64 chunk is
        // accepted into the session and only dropped (without emitting)
        // when `end` fails to decode the joined stream.
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(
            &cmd(
                "chunk",
                &[("id", "a"), ("seq", "0"), ("data", "not!base64!")],
            ),
            t,
            &mut sink,
        );
        assert_eq!(mgr.session_count(), 1);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(mgr.session_count(), 0);
        assert!(sink.html_requests.is_empty());
    }

    #[test]
    fn size_cap_drops_session_no_panic() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        let huge = "A".repeat(MAX_SESSION_DATA_SIZE + 1);
        mgr.handle(
            &cmd("chunk", &[("id", "a"), ("seq", "0"), ("data", &huge)]),
            t,
            &mut sink,
        );
        assert_eq!(mgr.session_count(), 0);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert!(sink.html_requests.is_empty());
    }

    #[test]
    fn eleventh_concurrent_begin_is_rejected() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        for i in 0..MAX_SESSIONS {
            mgr.handle(&cmd("begin", &[("id", &i.to_string())]), t, &mut sink);
        }
        assert_eq!(mgr.session_count(), MAX_SESSIONS);
        mgr.handle(&cmd("begin", &[("id", "overflow")]), t, &mut sink);
        assert_eq!(mgr.session_count(), MAX_SESSIONS);
        mgr.handle(&cmd("end", &[("id", "overflow")]), t, &mut sink);
        assert!(sink.html_requests.is_empty());
    }

    #[test]
    fn idle_session_evicted_after_timeout() {
        let mut mgr = HtmlViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t0 = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t0, &mut sink);
        assert_eq!(mgr.session_count(), 1);
        let later = t0 + SESSION_TIMEOUT + std::time::Duration::from_secs(1);
        mgr.evict_expired(later);
        assert_eq!(mgr.session_count(), 0);
        mgr.handle(&cmd("end", &[("id", "a")]), later, &mut sink);
        assert!(sink.html_requests.is_empty());
    }

    // ── AC-4: payload writer round trip, create-new + 0600 ──────────────

    #[test]
    fn payload_round_trips_through_json() {
        let request = HtmlRenderRequest {
            html: "<html><body>本文 🎉</body></html>".to_string(),
            basedir: Some("/home/me/docs".to_string()),
        };
        let payload = HtmlPayload::from_request(request);
        let path = write_payload(&payload).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let back = HtmlPayload::from_json(&contents).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(back.html, "<html><body>本文 🎉</body></html>");
        assert_eq!(back.basedir.as_deref(), Some("/home/me/docs"));
        assert_eq!(payload, back);
    }

    #[test]
    fn payload_basedir_omitted_when_none() {
        let request = HtmlRenderRequest {
            html: "<p>x</p>".to_string(),
            basedir: None,
        };
        let payload = HtmlPayload::from_request(request);
        let json = payload.to_json().unwrap();
        assert!(!json.contains("basedir"));
        let back = HtmlPayload::from_json(&json).unwrap();
        assert_eq!(back.basedir, None);
    }

    #[test]
    fn write_payload_uses_unique_create_new_paths() {
        let payload_a = HtmlPayload::from_request(HtmlRenderRequest {
            html: "a".to_string(),
            basedir: None,
        });
        let payload_b = HtmlPayload::from_request(HtmlRenderRequest {
            html: "b".to_string(),
            basedir: None,
        });
        let path_a = write_payload(&payload_a).unwrap();
        let path_b = write_payload(&payload_b).unwrap();
        assert_ne!(path_a, path_b, "temp payload paths must be unique");
        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);
    }

    #[cfg(unix)]
    #[test]
    fn write_payload_file_is_mode_0600() {
        use std::os::unix::fs::PermissionsExt as _;
        let payload = HtmlPayload::from_request(HtmlRenderRequest {
            html: "<p>secret</p>".to_string(),
            basedir: None,
        });
        let path = write_payload(&payload).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            mode & 0o777,
            0o600,
            "payload file must be create-new + owner-only (0600)"
        );
    }

    // ── AC-5: the child flag is a stable, assertable constant ───────────

    #[test]
    fn html_viewer_flag_is_the_agreed_child_flag() {
        assert_eq!(HTML_VIEWER_FLAG, "--html-viewer");
    }
}
