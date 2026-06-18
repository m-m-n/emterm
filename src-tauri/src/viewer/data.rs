//! `DataViewerSessions` — begin/chunk/end reassembly for the JSON/YAML
//! data viewer (Rust port of the WebView `DataViewerSessionManager`,
//! `src/data-viewer/session.ts`).
//!
//! Mirrors the Markdown session lifecycle (`markdown.rs`): chunks arrive
//! as raw base64 keyed by `seq`, the size/session/timeout caps match the
//! WebView build, and a successful `end` joins + decodes the stream once
//! and emits a [`DataRenderRequest`] to the [`ViewerSink`]. The `emterm
//! json` / `emterm yaml` CLIs never park on stdin (unlike interactive
//! `emterm markdown`), so there is no release-input path here.

use std::collections::HashMap;
use std::time::Instant;

use base64::Engine;

use super::markdown::{MAX_SESSION_DATA_SIZE, MAX_SESSIONS, SESSION_TIMEOUT};
use super::{ParsedCommand, ViewerSink};

/// Source format of a data-viewer document. Carried from the OSC `kind`
/// token through to the child viewer window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Json,
    Yaml,
}

impl DataFormat {
    /// Parse an OSC kind token. Only `json` / `yaml` route here.
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "json" => Some(Self::Json),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// Wire token used in the child viewer payload header.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
        }
    }
}

/// A completed JSON/YAML document ready to be displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRenderRequest {
    /// Fully reassembled, base64-decoded UTF-8 source text.
    pub text: String,
    /// Source format.
    pub format: DataFormat,
}

/// One in-flight data-viewer session keyed by its `id`.
#[derive(Debug)]
struct Session {
    format: DataFormat,
    /// Raw base64 chunk text indexed by `seq` (decoded once on `end`,
    /// same rationale as the Markdown sessions).
    chunks: HashMap<u64, String>,
    /// Cumulative encoded base64 length accepted so far (size cap).
    encoded_size: usize,
    last_activity: Instant,
}

/// Manages the begin/chunk/end lifecycle for JSON/YAML viewer sessions.
/// JSON and YAML sessions share one id namespace and the [`MAX_SESSIONS`]
/// budget (matches the WebView build's single `DataViewerSessionManager`).
#[derive(Debug, Default)]
pub struct DataViewerSessions {
    sessions: HashMap<String, Session>,
}

impl DataViewerSessions {
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

    /// Handle one parsed `json` / `yaml` command. On a successful `end`,
    /// a [`DataRenderRequest`] is pushed to `sink` via
    /// [`ViewerSink::emit_data`].
    pub fn handle(
        &mut self,
        format: DataFormat,
        cmd: &ParsedCommand,
        now: Instant,
        sink: &mut dyn ViewerSink,
    ) {
        match cmd.verb.as_str() {
            "begin" => self.handle_begin(format, cmd, now),
            "chunk" => self.handle_chunk(cmd, now),
            "end" => self.handle_end(cmd, sink),
            other => {
                log::warn!("data viewer: unknown verb {other:?}");
            }
        }
    }

    /// Drop sessions idle for longer than [`SESSION_TIMEOUT`].
    pub fn evict_expired(&mut self, now: Instant) {
        let before = self.sessions.len();
        self.sessions
            .retain(|_, s| now.duration_since(s.last_activity) <= SESSION_TIMEOUT);
        let dropped = before - self.sessions.len();
        if dropped > 0 {
            log::warn!("data viewer: dropped {dropped} timed-out session(s)");
        }
    }

    fn handle_begin(&mut self, format: DataFormat, cmd: &ParsedCommand, now: Instant) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("data viewer begin: missing id");
            return;
        };
        if self.sessions.len() >= MAX_SESSIONS {
            log::warn!("data viewer begin: max sessions ({MAX_SESSIONS}) reached, rejecting {id}");
            return;
        }
        self.sessions.insert(
            id.clone(),
            Session {
                format,
                chunks: HashMap::new(),
                encoded_size: 0,
                last_activity: now,
            },
        );
    }

    fn handle_chunk(&mut self, cmd: &ParsedCommand, now: Instant) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("data viewer chunk: missing id");
            return;
        };
        let Some(session) = self.sessions.get_mut(id) else {
            log::warn!("data viewer chunk: unknown session {id}");
            return;
        };
        let Some(seq_str) = cmd.params.get("seq") else {
            log::warn!("data viewer chunk: missing seq for {id}");
            return;
        };
        let Ok(seq) = seq_str.parse::<u64>() else {
            log::warn!("data viewer chunk: invalid seq {seq_str:?} for {id}");
            return;
        };
        let Some(data) = cmd.params.get("data") else {
            log::warn!("data viewer chunk: missing data for {id}");
            return;
        };

        if session.encoded_size.saturating_add(data.len()) > MAX_SESSION_DATA_SIZE {
            log::warn!("data viewer chunk: size cap exceeded for {id}, dropping session");
            self.sessions.remove(id);
            return;
        }

        session.encoded_size += data.len();
        session.chunks.insert(seq, data.clone());
        session.last_activity = now;
    }

    fn handle_end(&mut self, cmd: &ParsedCommand, sink: &mut dyn ViewerSink) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("data viewer end: missing id");
            return;
        };
        let Some(session) = self.sessions.remove(id) else {
            log::warn!("data viewer end: unknown session {id}");
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
                log::warn!("data viewer end: invalid base64 for {id}, dropping session");
                return;
            }
        };
        let text = match String::from_utf8(decoded) {
            Ok(s) => s,
            Err(_) => {
                log::warn!("data viewer end: invalid UTF-8 for {id}, dropping session");
                return;
            }
        };

        sink.emit_data(DataRenderRequest {
            text,
            format: session.format,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::CapturingSink;

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    }

    fn cmd(kind: &str, verb: &str, pairs: &[(&str, &str)]) -> ParsedCommand {
        ParsedCommand {
            kind: kind.to_string(),
            verb: verb.to_string(),
            params: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    fn run(events: Vec<(DataFormat, ParsedCommand)>) -> Vec<DataRenderRequest> {
        let mut sessions = DataViewerSessions::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        for (format, c) in events {
            sessions.handle(format, &c, now, &mut sink);
        }
        sink.data_requests
    }

    #[test]
    fn json_session_round_trips() {
        let out = run(vec![
            (DataFormat::Json, cmd("json", "begin", &[("id", "a")])),
            (
                DataFormat::Json,
                cmd(
                    "json",
                    "chunk",
                    &[("id", "a"), ("seq", "0"), ("data", &b64("{\"k\":1}"))],
                ),
            ),
            (DataFormat::Json, cmd("json", "end", &[("id", "a")])),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "{\"k\":1}");
        assert_eq!(out[0].format, DataFormat::Json);
    }

    #[test]
    fn yaml_session_round_trips() {
        let out = run(vec![
            (DataFormat::Yaml, cmd("yaml", "begin", &[("id", "y")])),
            (
                DataFormat::Yaml,
                cmd(
                    "yaml",
                    "chunk",
                    &[("id", "y"), ("seq", "0"), ("data", &b64("k: 1\n"))],
                ),
            ),
            (DataFormat::Yaml, cmd("yaml", "end", &[("id", "y")])),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "k: 1\n");
        assert_eq!(out[0].format, DataFormat::Yaml);
    }

    #[test]
    fn chunks_reassemble_in_seq_order() {
        // Split a base64 stream across two chunks out of order; the join
        // must be seq-sorted before decoding.
        let whole = b64("{\"long\":\"document\"}");
        let (first, second) = whole.split_at(8);
        let out = run(vec![
            (DataFormat::Json, cmd("json", "begin", &[("id", "a")])),
            (
                DataFormat::Json,
                cmd(
                    "json",
                    "chunk",
                    &[("id", "a"), ("seq", "1"), ("data", second)],
                ),
            ),
            (
                DataFormat::Json,
                cmd(
                    "json",
                    "chunk",
                    &[("id", "a"), ("seq", "0"), ("data", first)],
                ),
            ),
            (DataFormat::Json, cmd("json", "end", &[("id", "a")])),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "{\"long\":\"document\"}");
    }

    #[test]
    fn end_without_begin_emits_nothing() {
        let out = run(vec![(
            DataFormat::Json,
            cmd("json", "end", &[("id", "ghost")]),
        )]);
        assert!(out.is_empty());
    }

    #[test]
    fn invalid_base64_drops_session() {
        let out = run(vec![
            (DataFormat::Json, cmd("json", "begin", &[("id", "a")])),
            (
                DataFormat::Json,
                cmd(
                    "json",
                    "chunk",
                    &[("id", "a"), ("seq", "0"), ("data", "%%not-base64%%")],
                ),
            ),
            (DataFormat::Json, cmd("json", "end", &[("id", "a")])),
        ]);
        assert!(out.is_empty());
    }

    #[test]
    fn max_sessions_rejects_new_begin() {
        let mut sessions = DataViewerSessions::new();
        let mut sink = CapturingSink::default();
        let now = Instant::now();
        for i in 0..MAX_SESSIONS {
            sessions.handle(
                DataFormat::Json,
                &cmd("json", "begin", &[("id", &format!("s{i}"))]),
                now,
                &mut sink,
            );
        }
        assert_eq!(sessions.session_count(), MAX_SESSIONS);
        sessions.handle(
            DataFormat::Json,
            &cmd("json", "begin", &[("id", "overflow")]),
            now,
            &mut sink,
        );
        assert_eq!(sessions.session_count(), MAX_SESSIONS);
    }

    #[test]
    fn timed_out_session_is_evicted() {
        let mut sessions = DataViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t0 = Instant::now();
        sessions.handle(
            DataFormat::Yaml,
            &cmd("yaml", "begin", &[("id", "y")]),
            t0,
            &mut sink,
        );
        sessions.evict_expired(t0 + SESSION_TIMEOUT + std::time::Duration::from_secs(1));
        assert_eq!(sessions.session_count(), 0);
    }

    #[test]
    fn data_format_parses_known_kinds_only() {
        assert_eq!(DataFormat::parse("json"), Some(DataFormat::Json));
        assert_eq!(DataFormat::parse("yaml"), Some(DataFormat::Yaml));
        assert_eq!(DataFormat::parse("markdown"), None);
        assert_eq!(DataFormat::parse(""), None);
    }
}
