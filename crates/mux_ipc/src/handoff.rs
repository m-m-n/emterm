//! Versioned handoff document for mux daemon hot-upgrade (SPEC FR4, FR14;
//! IMPLEMENTATION.md Shared Components "Handoff document type" / "Handoff
//! schema version constant").
//!
//! This module owns the plain, serde-encodable description of everything a
//! successor daemon process needs after `execve` replaces the running
//! daemon: the session/window/pane tree, the session manager's ID
//! allocation counters, the incarnation token, and per-pane descriptor /
//! child-process bookkeeping. It deliberately depends on nothing from the
//! daemon binary — no PTY handles, no channels, no session-manager types —
//! so that `mux_ipc` stays below `src-tauri` in the layer structure.
//! Producing this document from live daemon state, and restoring it back
//! into one, is task0003's responsibility; this module only defines the
//! shape and the version-checked codec.
//!
//! The handoff schema version ([`HANDOFF_SCHEMA_VERSION`]) is versioned
//! independently of [`crate::protocol::PROTOCOL_VERSION`] (IMPLEMENTATION.md
//! D7): adding the handoff format, or changing it in the future, must never
//! move the wire protocol version a running GUI/CLI client negotiates at
//! Hello/Welcome time.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::protocol::AgentState;

/// Schema version of [`HandoffDocument`].
///
/// A monotonically increasing integer, bumped whenever the document's
/// on-wire shape changes incompatibly. Independent of
/// [`crate::protocol::PROTOCOL_VERSION`] — see the module docs.
pub const HANDOFF_SCHEMA_VERSION: u32 = 1;

/// Inclusive range of [`HandoffDocument`] schema versions this build can
/// restore.
///
/// Today the range contains only [`HANDOFF_SCHEMA_VERSION`]; a future build
/// widens it (e.g. `PREVIOUS_HANDOFF_SCHEMA_VERSION..=HANDOFF_SCHEMA_VERSION`)
/// so it can also restore its immediate predecessor's format.
/// [`decode_handoff_document`] rejects any document whose version falls
/// outside this range.
pub const SUPPORTED_HANDOFF_SCHEMA_VERSIONS: RangeInclusive<u32> =
    HANDOFF_SCHEMA_VERSION..=HANDOFF_SCHEMA_VERSION;

/// One pane's transferable state.
///
/// A pane recorded as `exited` carries no descriptor to adopt and no child
/// to reap: both `master_fd` and `child_pid` are `None` in that case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffPane {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<String>,
    pub title: Option<String>,
    /// Current agent-status state, mirroring the daemon-side
    /// `AgentStatus.state` field.
    pub agent_state: Option<AgentState>,
    /// Current agent-status name, mirroring the daemon-side
    /// `AgentStatus.name` field.
    pub agent_name: Option<String>,
    /// Current agent-status revision counter, mirroring the daemon-side
    /// `AgentStatus.revision` field.
    pub agent_revision: u64,
    /// Whether this pane's PTY had already exited before the handoff.
    pub exited: bool,
    /// The pane's shell child process id (D6: `Box<dyn portable_pty::Child>`
    /// cannot be reconstructed after `execve`, so restore reaps by pid
    /// instead of by child handle). `None` for a pane recorded as `exited`.
    pub child_pid: Option<u32>,
    /// Raw fd number of the pane's PTY master, captured after its
    /// `FD_CLOEXEC` flag was cleared so it survives `execve`. `None` for a
    /// pane recorded as `exited` — there is no descriptor to adopt.
    pub master_fd: Option<i32>,
    /// Raw scrollback bytes, captured while the pane's scrollback lock was
    /// held. May contain arbitrary bytes, including embedded zero bytes and
    /// sequences that are not valid UTF-8 (D8: restore rebuilds the shadow
    /// parser by replaying these bytes, not by carrying byte-exact parser
    /// state).
    pub scrollback: Vec<u8>,
}

/// One window's transferable state: its panes plus the window-scoped pane
/// ID allocator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffWindow {
    pub id: u32,
    pub name: String,
    pub active_pane_id: Option<u32>,
    /// This window's next-pane-id counter.
    pub next_pane_id: u32,
    pub panes: Vec<HandoffPane>,
}

/// One session's transferable state: its windows, their display order, and
/// the session-scoped window ID allocator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffSession {
    pub id: u32,
    pub name: String,
    /// Explicit window display ordering (mirrors `MuxSession::window_order`).
    pub window_order: Vec<u32>,
    pub active_window_id: Option<u32>,
    /// This session's next-window-id counter.
    pub next_window_id: u32,
    pub windows: Vec<HandoffWindow>,
}

/// The complete versioned handoff document: everything a successor daemon
/// process needs to resume serving without killing any pane's shell.
///
/// `schema_version` MUST remain the first field: [`decode_handoff_document`]
/// reads it ahead of the rest of the document so a version mismatch is
/// detected before attempting to decode the (possibly differently shaped)
/// remainder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffDocument {
    pub schema_version: u32,
    /// The session manager's incarnation token (D5), restored verbatim so
    /// already-running shells' `EMTERM_PANE_ID` values stay valid.
    pub incarnation: String,
    /// Raw fd number of the listen socket, captured after its
    /// `FD_CLOEXEC` flag was cleared so it survives `execve`.
    pub listen_fd: i32,
    /// The session manager's next-session-id counter.
    pub next_session_id: u32,
    /// The session manager's global next-pane-id counter.
    pub next_pane_id: u32,
    pub sessions: Vec<HandoffSession>,
}

/// Error returned by [`decode_handoff_document`] when the given bytes
/// cannot become a [`HandoffDocument`] this build can restore.
///
/// Distinct from a generic decode failure: [`Self::UnsupportedVersion`]
/// means the bytes were structurally readable but describe a schema this
/// build declares it cannot restore, versus [`Self::Malformed`], which
/// means the bytes could not be interpreted at all (or were too short to
/// even carry a version field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffDecodeError {
    /// The document's `schema_version` is not in
    /// [`SUPPORTED_HANDOFF_SCHEMA_VERSIONS`].
    UnsupportedVersion {
        found: u32,
        supported: RangeInclusive<u32>,
    },
    /// The bytes could not be decoded as a `HandoffDocument` at all.
    Malformed,
}

impl std::fmt::Display for HandoffDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { found, supported } => write!(
                f,
                "handoff document schema version {found} is not supported \
                 (this build supports {}..={})",
                supported.start(),
                supported.end()
            ),
            Self::Malformed => write!(f, "handoff document bytes are malformed"),
        }
    }
}

impl std::error::Error for HandoffDecodeError {}

/// Encode a [`HandoffDocument`] using the same mechanism the crate already
/// uses for control-message payloads (`bincode`, via
/// [`crate::protocol::MuxMessage::control`]).
pub fn encode_handoff_document(doc: &HandoffDocument) -> Vec<u8> {
    bincode::serialize(doc).expect("handoff document serialization")
}

/// Decode a [`HandoffDocument`], version-checked.
///
/// Reads `schema_version` first — `bincode`'s default (fixint, little
/// endian, trailing bytes allowed) encodes it as the leading four bytes of
/// any document sharing this type's field order, so this succeeds
/// independently of whatever the rest of the payload contains. If that
/// version falls outside [`SUPPORTED_HANDOFF_SCHEMA_VERSIONS`], the document
/// is rejected immediately: the full struct is never assembled, so nothing
/// is partially applied. Only once the version is confirmed supported is
/// the complete document decoded; any failure at that point (truncated or
/// corrupted bytes) is reported as [`HandoffDecodeError::Malformed`],
/// distinguishable from a version mismatch.
pub fn decode_handoff_document(bytes: &[u8]) -> Result<HandoffDocument, HandoffDecodeError> {
    let version: u32 = bincode::deserialize(bytes).map_err(|_| HandoffDecodeError::Malformed)?;
    if !SUPPORTED_HANDOFF_SCHEMA_VERSIONS.contains(&version) {
        return Err(HandoffDecodeError::UnsupportedVersion {
            found: version,
            supported: SUPPORTED_HANDOFF_SCHEMA_VERSIONS,
        });
    }
    bincode::deserialize(bytes).map_err(|_| HandoffDecodeError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pane(id: u32) -> HandoffPane {
        HandoffPane {
            id,
            cols: 80,
            rows: 24,
            cwd: Some("/home/user/project".to_string()),
            title: Some("zsh".to_string()),
            agent_state: Some(AgentState::Working),
            agent_name: Some("claude".to_string()),
            agent_revision: 3,
            exited: false,
            child_pid: Some(4242),
            master_fd: Some(11),
            // Arbitrary bytes, including an embedded zero byte and a
            // sequence that is not valid UTF-8, per the Test Notes.
            scrollback: vec![0x1b, b'[', b'2', b'J', 0x00, 0xff, 0xfe, b'o', b'k'],
        }
    }

    fn sample_document() -> HandoffDocument {
        HandoffDocument {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incarnation: "a1b2c3d4".to_string(),
            listen_fd: 3,
            next_session_id: 2,
            next_pane_id: 5,
            sessions: vec![
                HandoffSession {
                    id: 1,
                    name: "main".to_string(),
                    window_order: vec![1, 2],
                    active_window_id: Some(2),
                    next_window_id: 3,
                    windows: vec![
                        HandoffWindow {
                            id: 1,
                            name: "shell".to_string(),
                            active_pane_id: Some(1),
                            next_pane_id: 2,
                            panes: vec![sample_pane(1)],
                        },
                        HandoffWindow {
                            id: 2,
                            name: "logs".to_string(),
                            active_pane_id: Some(3),
                            next_pane_id: 4,
                            panes: vec![sample_pane(3), sample_pane(4)],
                        },
                    ],
                },
                HandoffSession {
                    id: 2,
                    name: "second".to_string(),
                    window_order: vec![1],
                    active_window_id: Some(1),
                    next_window_id: 2,
                    windows: vec![HandoffWindow {
                        id: 1,
                        name: "shell".to_string(),
                        active_pane_id: Some(5),
                        next_pane_id: 6,
                        panes: vec![sample_pane(5)],
                    }],
                },
            ],
        }
    }

    /// AC-4: the handoff document round-trips through encode → decode with
    /// the session/window/pane tree, ID counters, incarnation token,
    /// descriptor numbers, child process ids and scrollback bytes (including
    /// non-UTF-8 bytes and an embedded zero byte) all preserved
    /// byte-for-byte.
    #[test]
    fn test_handoff_document_round_trips_with_full_tree() {
        let doc = sample_document();
        let encoded = encode_handoff_document(&doc);
        let decoded = decode_handoff_document(&encoded).expect("decode should succeed");
        assert_eq!(decoded, doc);
    }

    /// AC-7: a document containing zero sessions round-trips successfully.
    #[test]
    fn test_empty_handoff_document_round_trips() {
        let doc = HandoffDocument {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incarnation: "deadbeef".to_string(),
            listen_fd: 3,
            next_session_id: 1,
            next_pane_id: 1,
            sessions: Vec::new(),
        };
        let encoded = encode_handoff_document(&doc);
        let decoded = decode_handoff_document(&encoded).expect("decode should succeed");
        assert_eq!(decoded, doc);
        assert!(decoded.sessions.is_empty());
    }

    /// A pane recorded as exited carries no descriptor to adopt: both
    /// `master_fd` and `child_pid` round-trip as `None`.
    #[test]
    fn test_handoff_document_round_trips_exited_pane_without_descriptor() {
        let exited_pane = HandoffPane {
            id: 9,
            cols: 80,
            rows: 24,
            cwd: None,
            title: None,
            agent_state: None,
            agent_name: None,
            agent_revision: 0,
            exited: true,
            child_pid: None,
            master_fd: None,
            scrollback: Vec::new(),
        };
        let doc = HandoffDocument {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incarnation: "cafef00d".to_string(),
            listen_fd: 3,
            next_session_id: 2,
            next_pane_id: 10,
            sessions: vec![HandoffSession {
                id: 1,
                name: "main".to_string(),
                window_order: vec![1],
                active_window_id: Some(1),
                next_window_id: 2,
                windows: vec![HandoffWindow {
                    id: 1,
                    name: "shell".to_string(),
                    active_pane_id: Some(9),
                    next_pane_id: 10,
                    panes: vec![exited_pane],
                }],
            }],
        };
        let encoded = encode_handoff_document(&doc);
        let decoded = decode_handoff_document(&encoded).expect("decode should succeed");
        assert_eq!(decoded, doc);
        let pane = &decoded.sessions[0].windows[0].panes[0];
        assert!(pane.exited);
        assert_eq!(pane.master_fd, None);
        assert_eq!(pane.child_pid, None);
    }

    /// AC-5: decoding a document whose schema version is outside the
    /// supported range fails with a version-specific error, distinguishable
    /// from a malformed-payload error.
    #[test]
    fn test_decode_handoff_document_rejects_version_outside_supported_range() {
        let doc = sample_document();
        let mut encoded = encode_handoff_document(&doc);
        // `schema_version` is the leading field; overwrite its first four
        // (fixint LE) bytes with a version this build does not support.
        let bogus_version: u32 = 9999;
        encoded[0..4].copy_from_slice(&bogus_version.to_le_bytes());

        let err = decode_handoff_document(&encoded).expect_err("should reject unsupported version");
        assert_eq!(
            err,
            HandoffDecodeError::UnsupportedVersion {
                found: bogus_version,
                supported: SUPPORTED_HANDOFF_SCHEMA_VERSIONS,
            }
        );
        assert_ne!(err, HandoffDecodeError::Malformed);
    }

    /// AC-5 (continued): a version-in-range but truncated/corrupted payload
    /// is reported as `Malformed`, never misread as an `UnsupportedVersion`.
    #[test]
    fn test_decode_handoff_document_distinguishes_malformed_from_unsupported_version() {
        let doc = sample_document();
        let encoded = encode_handoff_document(&doc);
        // Keep the (supported) leading version field, but truncate
        // everything after it.
        let truncated = &encoded[0..8];
        assert_eq!(
            decode_handoff_document(truncated),
            Err(HandoffDecodeError::Malformed)
        );

        // Too short to even carry a version field.
        assert_eq!(
            decode_handoff_document(&[0x01, 0x02]),
            Err(HandoffDecodeError::Malformed)
        );
    }

    /// Locks in the encoding assumption `decode_handoff_document` depends
    /// on: `schema_version` is encoded as the first four bytes, little
    /// endian, fixed-width (not varint).
    #[test]
    fn test_encoded_document_leads_with_little_endian_schema_version() {
        let doc = sample_document();
        let encoded = encode_handoff_document(&doc);
        assert_eq!(&encoded[0..4], &HANDOFF_SCHEMA_VERSION.to_le_bytes()[..]);
    }
}
