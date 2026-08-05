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
///
/// Bumped to 2 (task0004, SPEC FR6): [`HandoffPane`] gained three new
/// fields carrying the daemon-side inferred-clear latch's state across a
/// hot-upgrade boundary. Since `bincode`'s encoding is positional, a
/// version-1 document decoded against the version-2 struct would
/// misalign every field after the first affected pane rather than
/// merely omitting the new ones — the version gate in
/// [`decode_handoff_document`] is what turns that into a clean rejection
/// instead of silent corruption.
///
/// Bumped to 3 (mux-hot-upgrade-alt-screen task0002, SPEC FR3/FR4):
/// [`HandoffPane`] gained two new fields carrying the pane's
/// alternate-screen state (flag + formatted-contents dump) across a
/// hot-upgrade boundary, for the same positional-decode reason the
/// version-2 bump documents above.
pub const HANDOFF_SCHEMA_VERSION: u32 = 3;

/// Inclusive range of [`HandoffDocument`] schema versions this build can
/// restore.
///
/// Today the range contains only [`HANDOFF_SCHEMA_VERSION`]; a future build
/// widens it (e.g. `PREVIOUS_HANDOFF_SCHEMA_VERSION..=HANDOFF_SCHEMA_VERSION`)
/// so it can also restore its immediate predecessor's format.
/// [`decode_handoff_document`] rejects any document whose version falls
/// outside this range.
pub const SUPPORTED_HANDOFF_SCHEMA_VERSIONS: RangeInclusive<u32> = 1..=HANDOFF_SCHEMA_VERSION;

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
    /// This pane's inferred-clear latch state (task0004, SPEC FR6):
    /// mirrors the daemon-side `AgentStatusExitLatch`'s three state
    /// components (`state_parts()`/`from_state_parts()`) exactly, as
    /// primitives rather than the daemon-side type itself — `mux_ipc`
    /// depends on nothing from the daemon binary (module docs), so the
    /// latch type (which lives in `src-tauri`) cannot appear here.
    pub latch_armed: bool,
    /// See [`Self::latch_armed`].
    pub latch_command_ended: bool,
    /// See [`Self::latch_armed`].
    pub latch_generation: u64,
    /// True iff this pane's shadow parser was on the alternate screen at
    /// capture time (mux-hot-upgrade-alt-screen task0002, SPEC FR3/FR4).
    /// A version-1 or version-2-originated document always decodes this as
    /// `false` (NFR3: those schemas predate alt-screen tracking).
    pub alt_screen: bool,
    /// The shadow parser's formatted alternate-screen contents at capture
    /// time, present iff [`Self::alt_screen`] is true AND the dump did not
    /// exceed the D1 size cap (IMPLEMENTATION.md) — an over-cap capture
    /// keeps the flag true but stores this empty instead. Empty for a
    /// main-buffer pane, an exited pane, or a version-1/version-2-originated
    /// document.
    pub alt_screen_dump: Vec<u8>,
}

/// Version-1 shape of [`HandoffPane`] (pre-task0004): identical to the
/// current struct minus the three latch fields.
///
/// Kept only so [`decode_handoff_document`] can read a schema-version-1
/// document written by a pre-task0004 daemon during hot-upgrade and
/// upgrade it into the current [`HandoffPane`] with the latch fields
/// defaulted to disarmed (no Set was pending under the old binary, since
/// the latch didn't exist yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffPaneV1 {
    id: u32,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    title: Option<String>,
    agent_state: Option<AgentState>,
    agent_name: Option<String>,
    agent_revision: u64,
    exited: bool,
    child_pid: Option<u32>,
    master_fd: Option<i32>,
    scrollback: Vec<u8>,
}

impl From<HandoffPaneV1> for HandoffPaneV2 {
    fn from(v1: HandoffPaneV1) -> Self {
        HandoffPaneV2 {
            id: v1.id,
            cols: v1.cols,
            rows: v1.rows,
            cwd: v1.cwd,
            title: v1.title,
            agent_state: v1.agent_state,
            agent_name: v1.agent_name,
            agent_revision: v1.agent_revision,
            exited: v1.exited,
            child_pid: v1.child_pid,
            master_fd: v1.master_fd,
            scrollback: v1.scrollback,
            // A version-1 document predates the inferred-clear latch
            // (task0004): treat every pane as disarmed.
            latch_armed: false,
            latch_command_ended: false,
            latch_generation: 0,
        }
    }
}

/// Version-1 shape of [`HandoffWindow`]: identical to [`HandoffWindowV2`],
/// but its panes are [`HandoffPaneV1`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffWindowV1 {
    id: u32,
    name: String,
    active_pane_id: Option<u32>,
    next_pane_id: u32,
    panes: Vec<HandoffPaneV1>,
}

impl From<HandoffWindowV1> for HandoffWindowV2 {
    fn from(v1: HandoffWindowV1) -> Self {
        HandoffWindowV2 {
            id: v1.id,
            name: v1.name,
            active_pane_id: v1.active_pane_id,
            next_pane_id: v1.next_pane_id,
            panes: v1.panes.into_iter().map(HandoffPaneV2::from).collect(),
        }
    }
}

/// Version-1 shape of [`HandoffSession`]: identical to [`HandoffSessionV2`],
/// but its windows are [`HandoffWindowV1`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffSessionV1 {
    id: u32,
    name: String,
    window_order: Vec<u32>,
    active_window_id: Option<u32>,
    next_window_id: u32,
    windows: Vec<HandoffWindowV1>,
}

impl From<HandoffSessionV1> for HandoffSessionV2 {
    fn from(v1: HandoffSessionV1) -> Self {
        HandoffSessionV2 {
            id: v1.id,
            name: v1.name,
            window_order: v1.window_order,
            active_window_id: v1.active_window_id,
            next_window_id: v1.next_window_id,
            windows: v1.windows.into_iter().map(HandoffWindowV2::from).collect(),
        }
    }
}

/// Version-1 shape of [`HandoffDocument`]: identical to [`HandoffDocumentV2`],
/// but its sessions are [`HandoffSessionV1`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffDocumentV1 {
    schema_version: u32,
    incarnation: String,
    listen_fd: i32,
    next_session_id: u32,
    next_pane_id: u32,
    sessions: Vec<HandoffSessionV1>,
}

impl From<HandoffDocumentV1> for HandoffDocumentV2 {
    fn from(v1: HandoffDocumentV1) -> Self {
        HandoffDocumentV2 {
            schema_version: 2,
            incarnation: v1.incarnation,
            listen_fd: v1.listen_fd,
            next_session_id: v1.next_session_id,
            next_pane_id: v1.next_pane_id,
            sessions: v1.sessions.into_iter().map(HandoffSessionV2::from).collect(),
        }
    }
}

/// Version-2 shape of [`HandoffPane`] (pre-task0002, mux-hot-upgrade-alt-screen):
/// identical to the current struct minus the two alt-screen fields.
///
/// Kept only so [`decode_handoff_document`] can read a schema-version-2
/// document written by a pre-task0002 daemon during hot-upgrade and upgrade
/// it into the current [`HandoffPane`] with the alt-screen fields defaulted
/// to `false` / empty (no alt-screen state was ever recorded under the old
/// schema).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffPaneV2 {
    id: u32,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    title: Option<String>,
    agent_state: Option<AgentState>,
    agent_name: Option<String>,
    agent_revision: u64,
    exited: bool,
    child_pid: Option<u32>,
    master_fd: Option<i32>,
    scrollback: Vec<u8>,
    latch_armed: bool,
    latch_command_ended: bool,
    latch_generation: u64,
}

impl From<HandoffPaneV2> for HandoffPane {
    fn from(v2: HandoffPaneV2) -> Self {
        HandoffPane {
            id: v2.id,
            cols: v2.cols,
            rows: v2.rows,
            cwd: v2.cwd,
            title: v2.title,
            agent_state: v2.agent_state,
            agent_name: v2.agent_name,
            agent_revision: v2.agent_revision,
            exited: v2.exited,
            child_pid: v2.child_pid,
            master_fd: v2.master_fd,
            scrollback: v2.scrollback,
            latch_armed: v2.latch_armed,
            latch_command_ended: v2.latch_command_ended,
            latch_generation: v2.latch_generation,
            // A version-2 document predates alt-screen tracking (task0002):
            // treat every pane as main-buffer with no dump.
            alt_screen: false,
            alt_screen_dump: Vec::new(),
        }
    }
}

/// Version-2 shape of [`HandoffWindow`]: identical to the current struct,
/// but its panes are [`HandoffPaneV2`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffWindowV2 {
    id: u32,
    name: String,
    active_pane_id: Option<u32>,
    next_pane_id: u32,
    panes: Vec<HandoffPaneV2>,
}

impl From<HandoffWindowV2> for HandoffWindow {
    fn from(v2: HandoffWindowV2) -> Self {
        HandoffWindow {
            id: v2.id,
            name: v2.name,
            active_pane_id: v2.active_pane_id,
            next_pane_id: v2.next_pane_id,
            panes: v2.panes.into_iter().map(HandoffPane::from).collect(),
        }
    }
}

/// Version-2 shape of [`HandoffSession`]: identical to the current struct,
/// but its windows are [`HandoffWindowV2`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffSessionV2 {
    id: u32,
    name: String,
    window_order: Vec<u32>,
    active_window_id: Option<u32>,
    next_window_id: u32,
    windows: Vec<HandoffWindowV2>,
}

impl From<HandoffSessionV2> for HandoffSession {
    fn from(v2: HandoffSessionV2) -> Self {
        HandoffSession {
            id: v2.id,
            name: v2.name,
            window_order: v2.window_order,
            active_window_id: v2.active_window_id,
            next_window_id: v2.next_window_id,
            windows: v2.windows.into_iter().map(HandoffWindow::from).collect(),
        }
    }
}

/// Version-2 shape of [`HandoffDocument`]: identical to the current struct,
/// but its sessions are [`HandoffSessionV2`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HandoffDocumentV2 {
    schema_version: u32,
    incarnation: String,
    listen_fd: i32,
    next_session_id: u32,
    next_pane_id: u32,
    sessions: Vec<HandoffSessionV2>,
}

impl From<HandoffDocumentV2> for HandoffDocument {
    fn from(v2: HandoffDocumentV2) -> Self {
        HandoffDocument {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incarnation: v2.incarnation,
            listen_fd: v2.listen_fd,
            next_session_id: v2.next_session_id,
            next_pane_id: v2.next_pane_id,
            sessions: v2.sessions.into_iter().map(HandoffSession::from).collect(),
        }
    }
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
///
/// Dispatch per version: version 1 decodes through the full chain
/// (`HandoffDocumentV1` -> `HandoffDocumentV2` -> current); version 2
/// decodes through the `HandoffDocumentV2` -> current conversion; version
/// [`HANDOFF_SCHEMA_VERSION`] decodes directly.
pub fn decode_handoff_document(bytes: &[u8]) -> Result<HandoffDocument, HandoffDecodeError> {
    let version: u32 = bincode::deserialize(bytes).map_err(|_| HandoffDecodeError::Malformed)?;
    if !SUPPORTED_HANDOFF_SCHEMA_VERSIONS.contains(&version) {
        return Err(HandoffDecodeError::UnsupportedVersion {
            found: version,
            supported: SUPPORTED_HANDOFF_SCHEMA_VERSIONS,
        });
    }
    if version == 1 {
        let doc_v1: HandoffDocumentV1 =
            bincode::deserialize(bytes).map_err(|_| HandoffDecodeError::Malformed)?;
        let doc_v2: HandoffDocumentV2 = doc_v1.into();
        return Ok(HandoffDocument::from(doc_v2));
    }
    if version == 2 {
        let doc_v2: HandoffDocumentV2 =
            bincode::deserialize(bytes).map_err(|_| HandoffDecodeError::Malformed)?;
        return Ok(HandoffDocument::from(doc_v2));
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
            // task0004: a mid-flight latch (armed, command already ended,
            // non-zero generation) so the round-trip test below actually
            // exercises non-default values for all three components.
            latch_armed: true,
            latch_command_ended: true,
            latch_generation: 3,
            // task0002 (mux-hot-upgrade-alt-screen): an alt-screen pane
            // whose dump carries the same "arbitrary bytes" shape as
            // scrollback above (embedded zero byte, non-UTF-8 sequence), so
            // AC-1's round-trip actually exercises the byte-for-byte
            // preservation the criterion names.
            alt_screen: true,
            alt_screen_dump: vec![0x1b, b'[', b'2', b'J', 0x00, 0xff, 0xfe, b'a', b'l', b't'],
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
            // An exited pane's latch is disarmed (the default state) —
            // no Set was pending when the pane exited.
            latch_armed: false,
            latch_command_ended: false,
            latch_generation: 0,
            // AC-3: an exited pane has no live alternate-screen semantics
            // to carry.
            alt_screen: false,
            alt_screen_dump: Vec::new(),
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

    /// Hot-upgrade backward compatibility: a version-1 document (written by
    /// a pre-task0004 daemon, before the latch fields existed) decodes
    /// successfully into the current [`HandoffPane`] shape with the latch
    /// fields defaulted to disarmed.
    #[test]
    fn test_decode_handoff_document_upgrades_v1_document_with_disarmed_latch() {
        let pane_v1 = HandoffPaneV1 {
            id: 1,
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
            scrollback: vec![0x1b, b'[', b'2', b'J', 0x00, 0xff, 0xfe, b'o', b'k'],
        };
        let doc_v1 = HandoffDocumentV1 {
            schema_version: 1,
            incarnation: "a1b2c3d4".to_string(),
            listen_fd: 3,
            next_session_id: 2,
            next_pane_id: 2,
            sessions: vec![HandoffSessionV1 {
                id: 1,
                name: "main".to_string(),
                window_order: vec![1],
                active_window_id: Some(1),
                next_window_id: 2,
                windows: vec![HandoffWindowV1 {
                    id: 1,
                    name: "shell".to_string(),
                    active_pane_id: Some(1),
                    next_pane_id: 2,
                    panes: vec![pane_v1],
                }],
            }],
        };
        let encoded = bincode::serialize(&doc_v1).expect("v1 document serialization");

        let decoded = decode_handoff_document(&encoded).expect("v1 document should decode");

        assert_eq!(decoded.schema_version, HANDOFF_SCHEMA_VERSION);
        assert_eq!(decoded.incarnation, "a1b2c3d4");
        let pane = &decoded.sessions[0].windows[0].panes[0];
        assert_eq!(pane.id, 1);
        assert_eq!(pane.child_pid, Some(4242));
        assert!(!pane.latch_armed);
        assert!(!pane.latch_command_ended);
        assert_eq!(pane.latch_generation, 0);
        // AC-2: a version-1 document (which predates alt-screen tracking
        // too) decodes with the same alt defaults as a version-2 document —
        // flag false, dump empty — through the full V1 -> V2 -> current
        // chain.
        assert!(!pane.alt_screen);
        assert!(pane.alt_screen_dump.is_empty());
    }

    /// AC-2: a version-2-shaped document (written by a pre-task0002 daemon,
    /// before alt-screen tracking existed) decodes into the current
    /// [`HandoffPane`] shape with the alt-screen flag false, the dump empty,
    /// and every other field (including the version-2 latch fields)
    /// preserved — mirroring the existing V1 test's hand-build-encode-decode
    /// pattern (Test Notes).
    #[test]
    fn test_decode_handoff_document_upgrades_v2_document_with_no_alt_screen_state() {
        let pane_v2 = HandoffPaneV2 {
            id: 7,
            cols: 100,
            rows: 40,
            cwd: Some("/home/user/project".to_string()),
            title: Some("zsh".to_string()),
            agent_state: Some(AgentState::Working),
            agent_name: Some("claude".to_string()),
            agent_revision: 5,
            exited: false,
            child_pid: Some(4242),
            master_fd: Some(11),
            scrollback: vec![0x1b, b'[', b'2', b'J', 0x00, 0xff, 0xfe, b'o', b'k'],
            latch_armed: true,
            latch_command_ended: true,
            latch_generation: 3,
        };
        let doc_v2 = HandoffDocumentV2 {
            schema_version: 2,
            incarnation: "b2c3d4e5".to_string(),
            listen_fd: 3,
            next_session_id: 2,
            next_pane_id: 8,
            sessions: vec![HandoffSessionV2 {
                id: 1,
                name: "main".to_string(),
                window_order: vec![1],
                active_window_id: Some(1),
                next_window_id: 2,
                windows: vec![HandoffWindowV2 {
                    id: 1,
                    name: "shell".to_string(),
                    active_pane_id: Some(7),
                    next_pane_id: 8,
                    panes: vec![pane_v2],
                }],
            }],
        };
        let encoded = bincode::serialize(&doc_v2).expect("v2 document serialization");

        let decoded = decode_handoff_document(&encoded).expect("v2 document should decode");

        assert_eq!(decoded.schema_version, HANDOFF_SCHEMA_VERSION);
        assert_eq!(decoded.incarnation, "b2c3d4e5");
        let pane = &decoded.sessions[0].windows[0].panes[0];
        assert_eq!(pane.id, 7);
        assert_eq!((pane.cols, pane.rows), (100, 40));
        assert_eq!(pane.child_pid, Some(4242));
        // Every other field is preserved verbatim from the V2 document.
        assert!(pane.latch_armed);
        assert!(pane.latch_command_ended);
        assert_eq!(pane.latch_generation, 3);
        assert_eq!(
            pane.scrollback,
            vec![0x1b, b'[', b'2', b'J', 0x00, 0xff, 0xfe, b'o', b'k']
        );
        // AC-2: alt-screen state defaults to flag false, dump empty — a V2
        // document never recorded any.
        assert!(!pane.alt_screen);
        assert!(pane.alt_screen_dump.is_empty());
    }

    /// AC-1: `HANDOFF_SCHEMA_VERSION` is 3 and `SUPPORTED_HANDOFF_SCHEMA_VERSIONS`
    /// advertises 1..=3.
    #[test]
    fn test_handoff_schema_version_is_3_with_supported_range_1_to_3() {
        assert_eq!(HANDOFF_SCHEMA_VERSION, 3);
        assert_eq!(SUPPORTED_HANDOFF_SCHEMA_VERSIONS, 1..=3);
    }

    /// AC-1 (continued): the round-trip in
    /// `test_handoff_document_round_trips_with_full_tree` already proves the
    /// alt flag and dump (including non-UTF-8 bytes and an embedded zero
    /// byte, via `sample_pane`) survive encode -> decode byte-for-byte as
    /// part of the full document equality check; this test isolates that
    /// assertion on the alt-screen fields specifically.
    #[test]
    fn test_alt_screen_flag_and_dump_round_trip_byte_for_byte() {
        let doc = sample_document();
        let encoded = encode_handoff_document(&doc);
        let decoded = decode_handoff_document(&encoded).expect("decode should succeed");
        let pane = &decoded.sessions[0].windows[0].panes[0];
        assert!(pane.alt_screen);
        assert_eq!(
            pane.alt_screen_dump,
            vec![0x1b, b'[', b'2', b'J', 0x00, 0xff, 0xfe, b'a', b'l', b't'],
            "the alt-screen dump must round-trip byte-for-byte, including \
             non-UTF-8 bytes and an embedded zero byte"
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
