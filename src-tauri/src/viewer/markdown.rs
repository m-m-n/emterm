//! `MarkdownViewerSessions` — Rust port of the WebView
//! `MarkdownSessionManager` lifecycle (`src/markdown/session.ts`).
//!
//! Accumulates `begin` / `chunk` / `end` OSC commands into a complete
//! Markdown document. Enforces the same limits as the WebView build (max
//! concurrent sessions, cumulative size cap, idle timeout) and, on a
//! successful `end`, joins chunks in `seq` order, base64-decodes them to
//! UTF-8, and emits a [`RenderRequest`] to the caller-supplied
//! [`ViewerSink`]. No window is created here — Phase 4 provides the real
//! sink; tests use a capturing sink.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine;

use super::{MarkdownFormat, ParsedCommand, RenderRequest, ViewerSink};

/// Maximum concurrent Markdown sessions. Mirrors
/// `MarkdownSessionManager.MAX_SESSIONS` (SPEC ERR_MAX_SESSIONS).
pub const MAX_SESSIONS: usize = 10;

/// Cumulative cap on the base64 *text* length per session, in bytes.
/// Mirrors `MAX_CHUNK_DATA_SIZE` from the WebView build (SPEC ERR_SIZE).
pub const MAX_SESSION_DATA_SIZE: usize = 100 * 1024 * 1024;

/// Idle timeout — a session with no `chunk`/`end` activity for this long
/// is dropped on the next maintenance pass (SPEC ERR_TIMEOUT).
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(30);

/// A single in-flight Markdown session keyed by its `id`.
#[derive(Debug)]
struct Session {
    format: MarkdownFormat,
    #[allow(dead_code)] // carried for parity; not yet surfaced to the viewer.
    version: u32,
    basedir: Option<String>,
    /// Raw base64 chunk text indexed by `seq`. Stored *undecoded* (SPEC
    /// FR2): the base64 stream is concatenated in `seq` order and decoded
    /// exactly once on `end`, so chunks split mid-quantum or across a UTF-8
    /// char boundary reassemble correctly.
    chunks: HashMap<u64, String>,
    /// Cumulative size of the *encoded* base64 text accepted so far, used
    /// for the size cap (matches the WebView build, which sums `.length`
    /// of the base64 strings).
    encoded_size: usize,
    last_activity: Instant,
}

/// Manages the begin/chunk/end lifecycle for Markdown viewer sessions.
#[derive(Debug, Default)]
pub struct MarkdownViewerSessions {
    sessions: HashMap<String, Session>,
}

impl MarkdownViewerSessions {
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

    /// Handle one parsed `markdown` command, using `now` as the clock for
    /// timeout bookkeeping (injected for deterministic tests). On a
    /// successful `end`, a [`RenderRequest`] is pushed to `sink`.
    pub fn handle(&mut self, cmd: &ParsedCommand, now: Instant, sink: &mut dyn ViewerSink) {
        match cmd.verb.as_str() {
            "begin" => self.handle_begin(cmd, now),
            "chunk" => self.handle_chunk(cmd, now),
            "end" => self.handle_end(cmd, sink),
            other => {
                // SPEC ERR_BAD_VERB: unknown verb → warn + ignore.
                log::warn!("markdown viewer: unknown verb {other:?}");
            }
        }
    }

    /// Drop sessions idle for longer than [`SESSION_TIMEOUT`] (ERR_TIMEOUT).
    /// Called opportunistically on each drain pass by the spawner.
    pub fn evict_expired(&mut self, now: Instant) {
        let before = self.sessions.len();
        self.sessions
            .retain(|_, s| now.duration_since(s.last_activity) <= SESSION_TIMEOUT);
        let dropped = before - self.sessions.len();
        if dropped > 0 {
            log::warn!("markdown viewer: dropped {dropped} timed-out session(s)");
        }
    }

    fn handle_begin(&mut self, cmd: &ParsedCommand, now: Instant) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("markdown begin: missing id"); // ERR_NO_ID
            return;
        };
        if self.sessions.len() >= MAX_SESSIONS {
            // ERR_MAX_SESSIONS — reject begin + warn.
            log::warn!("markdown begin: max sessions ({MAX_SESSIONS}) reached, rejecting {id}");
            return;
        }
        let format = cmd
            .params
            .get("format")
            .map(|f| MarkdownFormat::parse(f))
            .unwrap_or_default();
        let version = cmd
            .params
            .get("version")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(1);
        let basedir = cmd.params.get("basedir").cloned().filter(|s| !s.is_empty());

        self.sessions.insert(
            id.clone(),
            Session {
                format,
                version,
                basedir,
                chunks: HashMap::new(),
                encoded_size: 0,
                last_activity: now,
            },
        );
    }

    fn handle_chunk(&mut self, cmd: &ParsedCommand, now: Instant) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("markdown chunk: missing id"); // ERR_NO_ID
            return;
        };
        let Some(session) = self.sessions.get_mut(id) else {
            log::warn!("markdown chunk: unknown session {id}");
            return;
        };
        let Some(seq_str) = cmd.params.get("seq") else {
            log::warn!("markdown chunk: missing seq for {id}");
            return;
        };
        let Ok(seq) = seq_str.parse::<u64>() else {
            log::warn!("markdown chunk: invalid seq {seq_str:?} for {id}");
            return;
        };
        let Some(data) = cmd.params.get("data") else {
            log::warn!("markdown chunk: missing data for {id}");
            return;
        };

        // ERR_SIZE: enforce the cumulative cap on the base64 text length
        // before storing, then drop the session (matches the WebView
        // build, which discards on overflow).
        if session.encoded_size.saturating_add(data.len()) > MAX_SESSION_DATA_SIZE {
            log::warn!("markdown chunk: size cap exceeded for {id}, dropping session");
            self.sessions.remove(id);
            return;
        }

        // SPEC FR2: store the *raw* base64 by `seq` and defer the decode to
        // `end`. Decoding per-chunk would corrupt the document when a chunk
        // boundary lands mid base64-quantum (not a multiple of 4 chars) or
        // splits a multi-byte UTF-8 char.
        session.encoded_size += data.len();
        session.chunks.insert(seq, data.clone());
        session.last_activity = now;
    }

    fn handle_end(&mut self, cmd: &ParsedCommand, sink: &mut dyn ViewerSink) {
        let Some(id) = cmd.params.get("id") else {
            log::warn!("markdown end: missing id"); // ERR_NO_ID
            return;
        };
        let Some(session) = self.sessions.remove(id) else {
            log::warn!("markdown end: unknown session {id}");
            return;
        };

        // SPEC FR2: concatenate the raw base64 chunks in ascending `seq`
        // order, then base64-decode + UTF-8-convert ONCE over the whole
        // stream. Reserve capacity upfront using the encoded size as an
        // upper bound (the base64 text is always ≥ the decoded bytes).
        let mut pairs: Vec<(u64, String)> = session.chunks.into_iter().collect();
        pairs.sort_unstable_by_key(|(seq, _)| *seq);
        let mut encoded = String::with_capacity(session.encoded_size);
        for (_, b64) in pairs {
            encoded.push_str(&b64);
        }

        // ERR_B64: malformed base64 → drop the session without emitting.
        let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded.as_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                log::warn!("markdown end: invalid base64 for {id}, dropping session");
                return;
            }
        };
        // Invalid UTF-8 → drop the session without emitting.
        let markdown = match String::from_utf8(decoded) {
            Ok(s) => s,
            Err(_) => {
                log::warn!("markdown end: invalid UTF-8 for {id}, dropping session");
                return;
            }
        };

        sink.emit(RenderRequest {
            markdown,
            format: session.format,
            basedir: session.basedir,
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

    fn cmd(verb: &str, pairs: &[(&str, &str)]) -> ParsedCommand {
        ParsedCommand {
            kind: "markdown".to_string(),
            verb: verb.to_string(),
            params: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn begin_chunk_end_joins_in_seq_order_and_decodes() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(
            &cmd("begin", &[("id", "a"), ("format", "gfm")]),
            t,
            &mut sink,
        );
        mgr.handle(
            &cmd(
                "chunk",
                &[("id", "a"), ("seq", "0"), ("data", &b64("Hello "))],
            ),
            t,
            &mut sink,
        );
        mgr.handle(
            &cmd(
                "chunk",
                &[("id", "a"), ("seq", "1"), ("data", &b64("World"))],
            ),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);

        assert_eq!(sink.requests.len(), 1);
        assert_eq!(sink.requests[0].markdown, "Hello World");
        assert_eq!(sink.requests[0].format, MarkdownFormat::Gfm);
        assert_eq!(mgr.session_count(), 0);
    }

    #[test]
    fn out_of_order_chunks_are_reordered_by_seq() {
        // SPEC FR2: chunks store RAW base64 and are decoded once on `end`.
        // To exercise reassembly we encode the WHOLE "ABC" document and
        // split the base64 across chunks, then feed them out of `seq` order.
        let encoded = b64("ABC");
        let third = encoded.len() / 3;
        let (c0, rest) = encoded.split_at(third);
        let (c1, c2) = rest.split_at(third);

        let mut mgr = MarkdownViewerSessions::new();
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
        assert_eq!(sink.requests[0].markdown, "ABC");
    }

    #[test]
    fn default_format_is_commonmark() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(sink.requests[0].format, MarkdownFormat::CommonMark);
    }

    #[test]
    fn empty_session_emits_empty_document() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(sink.requests.len(), 1);
        assert_eq!(sink.requests[0].markdown, "");
    }

    #[test]
    fn basedir_is_carried_into_request() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(
            &cmd("begin", &[("id", "a"), ("basedir", "/home/me/docs")]),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert_eq!(sink.requests[0].basedir.as_deref(), Some("/home/me/docs"));
    }

    #[test]
    fn eleventh_concurrent_begin_is_rejected() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        for i in 0..MAX_SESSIONS {
            mgr.handle(&cmd("begin", &[("id", &i.to_string())]), t, &mut sink);
        }
        assert_eq!(mgr.session_count(), MAX_SESSIONS);
        mgr.handle(&cmd("begin", &[("id", "overflow")]), t, &mut sink);
        // Rejected: still at the cap, and no overflow session exists.
        assert_eq!(mgr.session_count(), MAX_SESSIONS);
        mgr.handle(&cmd("end", &[("id", "overflow")]), t, &mut sink);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn size_cap_drops_session_no_panic() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
        // A single oversized base64 chunk exceeds the cap.
        let huge = "A".repeat(MAX_SESSION_DATA_SIZE + 1);
        mgr.handle(
            &cmd("chunk", &[("id", "a"), ("seq", "0"), ("data", &huge)]),
            t,
            &mut sink,
        );
        // Session dropped; a subsequent end is a no-op (unknown session).
        assert_eq!(mgr.session_count(), 0);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn malformed_base64_drops_session_no_panic() {
        // SPEC FR2: decode is deferred to `end`, so a malformed-base64
        // chunk is accepted into the session and the session is only dropped
        // (without emitting) when `end` fails to decode the joined stream.
        let mut mgr = MarkdownViewerSessions::new();
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
        // Session still in-flight (decode happens at `end`).
        assert_eq!(mgr.session_count(), 1);
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);
        // end dropped the session without emitting.
        assert_eq!(mgr.session_count(), 0);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn base64_split_at_non_4_boundary_reassembles_correctly() {
        // SPEC FR2: the base64 stream may be split anywhere, including
        // mid-quantum (not a multiple of 4 chars) and across a multi-byte
        // UTF-8 char boundary. Encode the WHOLE document once, then split
        // the resulting base64 text at offsets that are NOT multiples of 4
        // to prove the decode is performed over the concatenated stream.
        let doc = "本文 🎉 with mixed ASCII + multibyte café";
        let encoded = b64(doc);
        // Pick split offsets that are deliberately off the 4-char quantum.
        assert!(encoded.len() > 10, "fixture too short to split");
        let p1 = 3; // 3 % 4 != 0
        let p2 = 9; // 9 % 4 != 0
        let (c0, rest) = encoded.split_at(p1);
        let (c1, c2) = rest.split_at(p2 - p1);

        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t, &mut sink);
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
        mgr.handle(
            &cmd("chunk", &[("id", "a"), ("seq", "2"), ("data", c2)]),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[("id", "a")]), t, &mut sink);

        assert_eq!(sink.requests.len(), 1);
        assert_eq!(sink.requests[0].markdown, doc);
    }

    #[test]
    fn missing_id_is_ignored_no_panic() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("begin", &[("format", "gfm")]), t, &mut sink);
        mgr.handle(
            &cmd("chunk", &[("seq", "0"), ("data", &b64("x"))]),
            t,
            &mut sink,
        );
        mgr.handle(&cmd("end", &[]), t, &mut sink);
        assert_eq!(mgr.session_count(), 0);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn unknown_verb_is_ignored_no_panic() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t = Instant::now();
        mgr.handle(&cmd("frobnicate", &[("id", "a")]), t, &mut sink);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn idle_session_evicted_after_timeout() {
        let mut mgr = MarkdownViewerSessions::new();
        let mut sink = CapturingSink::default();
        let t0 = Instant::now();
        mgr.handle(&cmd("begin", &[("id", "a")]), t0, &mut sink);
        assert_eq!(mgr.session_count(), 1);
        let later = t0 + SESSION_TIMEOUT + Duration::from_secs(1);
        mgr.evict_expired(later);
        assert_eq!(mgr.session_count(), 0);
        // end after eviction is a no-op.
        mgr.handle(&cmd("end", &[("id", "a")]), later, &mut sink);
        assert!(sink.requests.is_empty());
    }

    #[test]
    fn interleaved_sessions_stay_independent() {
        let mut mgr = MarkdownViewerSessions::new();
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
        assert_eq!(sink.requests.len(), 2);
        assert_eq!(sink.requests[0].markdown, "BB");
        assert_eq!(sink.requests[1].markdown, "AA");
    }
}
