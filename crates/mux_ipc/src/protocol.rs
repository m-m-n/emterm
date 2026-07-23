//! IPC message types and frame format for mux daemon communication.
//!
//! Frame format: [length: u32][type: u8][pane_id: u32][payload: variable]
//! - length: remaining bytes after the length field (= 5 + payload_len)
//! - PTY data uses raw bytes payload
//! - Control messages use bincode-serialized payload

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

/// Protocol version for handshake compatibility check.
///
/// Bumped from 1 to 2 for the mux agent-status / agent-API message
/// additions (`AgentStatusUpdate`, `ReadPane`/`ReadPaneResult`,
/// `SendText`/`SendTextResult`, `WaitAgentState`/`WaitAgentStateResult`,
/// `AgentApiError`). No existing message's encoded bytes changed.
///
/// The handshake path (`HelloMsg::protocol_version` vs this constant,
/// checked in `mux/ipc/connection.rs`) rejects a mismatched client
/// cleanly via `WelcomeMsg::Rejected` — there is no silent compatibility
/// shim between protocol versions on the daemon side.
///
/// Client-side recovery (task0010 rework, strategy B): a hard version bump
/// alone would strand a long-lived old daemon after an eMterm upgrade, since
/// daemon discovery (`ensure_daemon_running`) was presence-based and never
/// probed compatibility. `mux/daemon.rs` now performs a real handshake
/// before trusting an already-running daemon; on a version mismatch it
/// retries with [`PREVIOUS_PROTOCOL_VERSION`] (which the older daemon
/// accepts) and sends a version-tolerant `Shutdown`, then relaunches a
/// current-version daemon. See IMPLEMENTATION.md "Old GUI × new daemon
/// pairing".
pub const PROTOCOL_VERSION: u32 = 2;

/// The protocol version immediately preceding [`PROTOCOL_VERSION`].
///
/// Used only for the client-side legacy-daemon recovery handshake retry
/// (`mux/daemon.rs::recover_from_legacy_daemon` /
/// `shutdown_daemon_any_version`, task0010 rework): a v2 client that meets a
/// daemon rejecting its v2 Hello retries with this version so an adjacent
/// older daemon accepts the connection and can be sent a `Shutdown`.
/// Deliberately supports only one version back — recovering a daemon more
/// than one bump behind is out of scope (see task0010's plan "Out of
/// Scope").
pub const PREVIOUS_PROTOCOL_VERSION: u32 = PROTOCOL_VERSION - 1;

/// APC prefix for identifying emterm mux APC sequences.
pub const APC_PREFIX: &str = "emterm-mux;";

/// APC introducer: ESC _
const APC_START: &str = "\x1b_";

/// APC string terminator: ESC \
const APC_ST: &str = "\x1b\\";

/// OSC parameter for emterm mux messages.
pub const MUX_OSC_PARAM: u16 = 9999;

/// Plaintext transport prefix for mux messages on the Windows ConPTY input
/// direction (`EMUX;<base64>\r`, where APC/OSC escapes do not survive ConPTY
/// input and a raw LF is dropped under `PSEUDOCONSOLE_WIN32_INPUT_MODE`).
/// The bridge parser also accepts LF / CRLF / LFCR for resilience. Kept here
/// alongside `APC_PREFIX` and `MUX_OSC_PARAM` so all three mux transport
/// markers share one SSOT.
pub const PLAINTEXT_PREFIX: &[u8] = b"EMUX;";

/// OSC introducer: ESC ]
const OSC_START: &str = "\x1b]";

/// Maximum IPC frame size (16MB) to prevent OOM.
pub const MAX_FRAME_LENGTH: usize = 16 * 1024 * 1024;

/// Message type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    PtyOutput = 0x01,
    PtyInput = 0x02,
    Hello = 0x03,
    Welcome = 0x04,
    CreatePane = 0x05,
    PaneCreated = 0x06,
    DestroyPane = 0x07,
    Resize = 0x08,
    Attach = 0x09,
    Detach = 0x0A,
    Detached = 0x0B,
    Snapshot = 0x0C,
    SnapshotRestore = 0x0D,
    SessionList = 0x0E,
    Error = 0x0F,
    PtyExited = 0x10,
    // Phase 3+ message types
    CreateWindow = 0x12,
    SwitchWindow = 0x13,
    RenameWindow = 0x14,
    DestroyWindow = 0x15,
    StatusUpdate = 0x16,
    RequestStatusUpdate = 0x17,
    Shutdown = 0x18,
    RequestPaneSnapshot = 0x19,
    MoveWindow = 0x1A,
    SetVisibility = 0x1B,
    /// Daemon-originated desktop notification (OSC 9) detected on a Detached
    /// pane. Forwarded to the GUI client, which fires the OS notification.
    Notify = 0x1C,
    /// Daemon → GUI unsolicited push: a mux pane's agent status changed (or
    /// is being restated after snapshot/reattach with `replay_derived`).
    AgentStatusUpdate = 0x1D,
    /// Request: read the last N lines of a mux pane's visible content.
    ReadPane = 0x1E,
    /// Response to `ReadPane`.
    ReadPaneResult = 0x1F,
    /// Request: send text (raw bytes) to a mux pane's PTY.
    SendText = 0x20,
    /// Response to `SendText`.
    SendTextResult = 0x21,
    /// Request: block until a mux pane's agent state matches a target set.
    WaitAgentState = 0x22,
    /// Response to `WaitAgentState`.
    WaitAgentStateResult = 0x23,
    /// Structured error response shared by `ReadPane` / `SendText` /
    /// `WaitAgentState`.
    AgentApiError = 0x24,
}

impl MessageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::PtyOutput),
            0x02 => Some(Self::PtyInput),
            0x03 => Some(Self::Hello),
            0x04 => Some(Self::Welcome),
            0x05 => Some(Self::CreatePane),
            0x06 => Some(Self::PaneCreated),
            0x07 => Some(Self::DestroyPane),
            0x08 => Some(Self::Resize),
            0x09 => Some(Self::Attach),
            0x0A => Some(Self::Detach),
            0x0B => Some(Self::Detached),
            0x0C => Some(Self::Snapshot),
            0x0D => Some(Self::SnapshotRestore),
            0x0E => Some(Self::SessionList),
            0x0F => Some(Self::Error),
            0x10 => Some(Self::PtyExited),
            0x12 => Some(Self::CreateWindow),
            0x13 => Some(Self::SwitchWindow),
            0x14 => Some(Self::RenameWindow),
            0x15 => Some(Self::DestroyWindow),
            0x16 => Some(Self::StatusUpdate),
            0x17 => Some(Self::RequestStatusUpdate),
            0x18 => Some(Self::Shutdown),
            0x19 => Some(Self::RequestPaneSnapshot),
            0x1A => Some(Self::MoveWindow),
            0x1B => Some(Self::SetVisibility),
            0x1C => Some(Self::Notify),
            0x1D => Some(Self::AgentStatusUpdate),
            0x1E => Some(Self::ReadPane),
            0x1F => Some(Self::ReadPaneResult),
            0x20 => Some(Self::SendText),
            0x21 => Some(Self::SendTextResult),
            0x22 => Some(Self::WaitAgentState),
            0x23 => Some(Self::WaitAgentStateResult),
            0x24 => Some(Self::AgentApiError),
            _ => None,
        }
    }
}

/// Client type for handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientType {
    /// GUI client: full data path (PTY I/O + control)
    Gui,
    /// CLI client: control only (session list, kill, detach notification)
    Cli,
}

/// Handshake request from client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMsg {
    pub client_type: ClientType,
    pub protocol_version: u32,
}

/// Per-window metadata for IPC messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub name: String,
    pub active_pane_id: u32,
}

/// Session info returned in Welcome message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub window_count: u32,
    pub pane_count: u32,
    /// Index of the active window (0-based) within the ordered window list.
    #[serde(default)]
    pub active_window_index: u32,
    /// Per-window details for target resolution.
    #[serde(default)]
    pub windows: Vec<WindowInfo>,
}

/// Handshake response from daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WelcomeMsg {
    Accepted {
        server_version: u32,
        sessions: Vec<SessionInfo>,
    },
    Rejected {
        reason: String,
    },
}

/// Best-effort parse of the daemon's reported protocol version out of a
/// `WelcomeMsg::Rejected` reason string produced by the version-mismatch
/// path in `mux/ipc/connection.rs`
/// (`"Protocol version mismatch: client={client}, server={server}"`).
///
/// This is deliberately NOT part of the `WelcomeMsg` wire shape: an older
/// daemon's `Rejected { reason }` bincode payload must decode against the
/// CURRENT `WelcomeMsg` definition unchanged (bincode has no
/// forward/backward field tolerance), so the recovery path
/// (task0010 rework) reads the server version out of the free-form reason
/// text instead of adding a structured field. Returns `None` for any other
/// reason text (a rejection for a different cause, or a future daemon that
/// changes this wording) — callers must treat that as "version unknown",
/// never panic, and fall back to a generic message (AC-3).
pub fn parse_rejected_server_version(reason: &str) -> Option<u32> {
    let after_marker = reason.rsplit_once("server=")?.1;
    let digits: String = after_marker
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// Resize request for a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeMsg {
    pub cols: u16,
    pub rows: u16,
}

/// PTY process exit notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyExitedMsg {
    pub exit_code: Option<u32>,
}

/// Error notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMsg {
    pub message: String,
}

/// Attach request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachMsg {
    pub session_id: u32,
}

/// Status update pushed from daemon to GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusUpdateMsg {
    pub left: String,
    pub right: String,
}

/// Rename window request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameWindowMsg {
    pub name: String,
}

/// Desktop-notification request pushed from daemon to GUI.
///
/// Carries the OSC 9 message body recognized on a Detached pane. The GUI
/// client fires the OS notification (permission-gated). Decoded on the
/// frontend as a bincode `String` (u64 LE length + UTF-8 bytes), matching
/// the existing `RenameWindowMsg` wire shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyMsg {
    pub message: String,
}

/// SetVisibility payload (1 byte: 0x01 = visible, 0x00 = hidden).
///
/// Carried as a raw 1-byte payload (NOT bincode) so the wire shape matches
/// the frontend `MuxClient.sendSetVisibility` encoding without requiring
/// any deserializer round-trip on the daemon side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetVisibilityPayload {
    pub visible: bool,
}

impl SetVisibilityPayload {
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let byte = *payload.first()?;
        Some(Self { visible: byte != 0 })
    }

    pub fn to_payload(self) -> Vec<u8> {
        vec![if self.visible { 0x01 } else { 0x00 }]
    }
}

/// Move window request (reorder within a session).
///
/// `target_index` is the 0-based position in `MuxSession::window_order`
/// the window should occupy after the move. The daemon clamps out-of-range
/// values to the valid range `[0, window_order.len() - 1]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveWindowMsg {
    pub target_index: u32,
}

/// Payload for CreateWindow message.
/// Carries optional window name and initial command.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateWindowPayload {
    pub name: Option<String>,
    pub command: Option<String>,
}

/// A complete IPC message with header and payload.
#[derive(Debug, Clone)]
pub struct MuxMessage {
    pub msg_type: MessageType,
    pub pane_id: u32,
    pub payload: Vec<u8>,
}

impl MuxMessage {
    /// Create a message with raw bytes payload (for PTY data).
    pub fn pty_output(pane_id: u32, data: Vec<u8>) -> Self {
        Self {
            msg_type: MessageType::PtyOutput,
            pane_id,
            payload: data,
        }
    }

    /// Create a message with raw bytes payload (for keyboard input).
    pub fn pty_input(pane_id: u32, data: Vec<u8>) -> Self {
        Self {
            msg_type: MessageType::PtyInput,
            pane_id,
            payload: data,
        }
    }

    /// Create a snapshot-reply message (`MessageType::Snapshot`).
    ///
    /// Used by the daemon connection drain when emitting a snapshot-kind
    /// chunk produced by `handle_request_pane_snapshot`. The payload is the
    /// raw snapshot bytes (clear+home prefix, scrollback, shadow screen);
    /// no `MessageType` change is introduced — `Snapshot = 0x0C` already
    /// exists and the client's `apply_mux_message::Snapshot|SnapshotRestore`
    /// arm dispatches it to `build_from_snapshot` + `scrollback_bypass`.
    pub fn snapshot(pane_id: u32, data: Vec<u8>) -> Self {
        Self {
            msg_type: MessageType::Snapshot,
            pane_id,
            payload: data,
        }
    }

    /// Create a control message with bincode-serialized payload.
    pub fn control<T: Serialize>(msg_type: MessageType, pane_id: u32, data: &T) -> Self {
        Self {
            msg_type,
            pane_id,
            payload: bincode::serialize(data).expect("control message serialization"),
        }
    }

    /// Serialize this message into a frame: [type: u8][pane_id: u32][payload]
    /// The length prefix is handled by the codec layer.
    pub fn to_frame_body(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(5 + self.payload.len());
        buf.push(self.msg_type as u8);
        buf.extend_from_slice(&self.pane_id.to_le_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Parse a frame body into a MuxMessage.
    pub fn from_frame_body(body: &[u8]) -> Option<Self> {
        if body.len() < 5 {
            return None;
        }
        let msg_type = MessageType::from_u8(body[0])?;
        let pane_id = u32::from_le_bytes([body[1], body[2], body[3], body[4]]);
        let payload = body[5..].to_vec();
        Some(Self {
            msg_type,
            pane_id,
            payload,
        })
    }

    /// Deserialize control message payload.
    pub fn decode_payload<T: for<'a> Deserialize<'a>>(&self) -> Option<T> {
        bincode::deserialize(&self.payload).ok()
    }

    /// Encode this message as an APC escape sequence string.
    ///
    /// Format: `ESC _ emterm-mux;<base64(frame_body)> ESC \`
    pub fn to_apc(&self) -> String {
        let body = self.to_frame_body();
        let encoded = BASE64.encode(&body);
        format!("{}{}{}{}", APC_START, APC_PREFIX, encoded, APC_ST)
    }

    /// Encode this message as an OSC 9999 escape sequence string.
    ///
    /// Format: `ESC ] 9999 ; emterm-mux;<base64(frame_body)> ESC \`
    /// Used as fallback transport when ConPTY strips APC sequences.
    pub fn to_osc(&self) -> String {
        let body = self.to_frame_body();
        let encoded = BASE64.encode(&body);
        format!(
            "{}{};{}{}{}",
            OSC_START, MUX_OSC_PARAM, APC_PREFIX, encoded, APC_ST
        )
    }

    /// Encode this message as a plaintext (escape-free) sequence string.
    ///
    /// Format: `EMUX;<base64(frame_body)>\r`
    ///
    /// The mux GUI→bridge direction on Windows passes through ConPTY's input
    /// processing, which silently strips APC and OSC escape sequences. This
    /// printable-ASCII envelope survives intact. The bridge's
    /// `StdinApcParser` recognizes it alongside APC and OSC 9999, so the
    /// daemon protocol payload is unchanged — only the on-wire framing
    /// differs from `to_apc` / `to_osc`.
    ///
    /// The terminator is CR (`\r`), not LF. portable-pty 0.8 opens ConPTY
    /// with `PSEUDOCONSOLE_WIN32_INPUT_MODE` (see
    /// `pty::input::encode_backspace_win32` for the parallel case), and
    /// raw LF written to that channel is not delivered as a real key event
    /// — the message would otherwise stall at the bridge with the prefix
    /// matched but no terminator ever arriving. CR rides through as
    /// `VK_RETURN` reliably, and the bridge parser accepts CR / LF / CRLF
    /// interchangeably.
    pub fn to_plaintext(&self) -> String {
        let body = self.to_frame_body();
        let encoded = BASE64.encode(&body);
        // PLAINTEXT_PREFIX is `b"EMUX;"`, guaranteed ASCII; reuse the SSOT
        // instead of re-stating the literal so a future prefix change
        // propagates to both encoder and parser.
        let prefix = std::str::from_utf8(PLAINTEXT_PREFIX).expect("PLAINTEXT_PREFIX is ASCII");
        format!("{}{}\r", prefix, encoded)
    }

    /// Decode an APC payload string into a MuxMessage.
    ///
    /// The `payload` parameter is the content between `ESC _` and `ESC \`,
    /// which must start with the `emterm-mux;` prefix.
    pub fn from_apc(payload: &str) -> Result<Self, ApcDecodeError> {
        let b64 = payload
            .strip_prefix(APC_PREFIX)
            .ok_or(ApcDecodeError::MissingPrefix)?;
        let bytes = BASE64
            .decode(b64)
            .map_err(|_| ApcDecodeError::InvalidBase64)?;
        Self::from_frame_body(&bytes).ok_or(ApcDecodeError::InvalidFrameBody)
    }
}

/// Errors that can occur when decoding an APC payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApcDecodeError {
    /// Payload does not start with `emterm-mux;`.
    MissingPrefix,
    /// Base64 decoding failed.
    InvalidBase64,
    /// Frame body is invalid (too short or unknown message type).
    InvalidFrameBody,
}

impl std::fmt::Display for ApcDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPrefix => write!(f, "missing emterm-mux; prefix"),
            Self::InvalidBase64 => write!(f, "invalid base64 encoding"),
            Self::InvalidFrameBody => write!(f, "invalid frame body"),
        }
    }
}

impl std::error::Error for ApcDecodeError {}

/// Local mirror of the core agent-status module's state enum
/// (`src-tauri/src/agent_status.rs`). `mux_ipc` must not depend on the
/// binary crate, so this type owns its own serde representation; the
/// lowercase string values (`idle`/`working`/`blocked`/`done`) are the
/// wire contract shared between the two modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Idle,
    Working,
    Blocked,
    Done,
}

/// Daemon → GUI unsolicited push: a mux pane's agent status changed, or is
/// being restated after a snapshot/reattach (`replay_derived: true`, in
/// which case the receiver must apply it silently — no transition event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusUpdateMsg {
    pub pane_id: u32,
    pub public_pane_id: String,
    pub state: Option<AgentState>,
    pub name: Option<String>,
    pub revision: u64,
    pub replay_derived: bool,
}

/// Request: read the last `lines` lines of a mux pane's visible content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPaneMsg {
    pub public_pane_id: String,
    pub lines: u32,
}

/// Response to `ReadPane`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPaneResultMsg {
    pub text: String,
}

/// Request: send text (raw bytes) to a mux pane's PTY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextMsg {
    pub public_pane_id: String,
    pub bytes: Vec<u8>,
}

/// Response to `SendText`: the pane's revision observed immediately before
/// the successful PTY write (the "watermark").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextResultMsg {
    pub revision_watermark: u64,
}

/// Request: block until a mux pane's agent state is a member of `states`
/// (and, when `after_revision` is given, the pane's revision exceeds it),
/// or until `timeout_ms` elapses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentStateMsg {
    pub public_pane_id: String,
    pub states: Vec<AgentState>,
    pub timeout_ms: u64,
    pub after_revision: Option<u64>,
}

/// Response to `WaitAgentState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentStateResultMsg {
    pub state: AgentState,
    pub revision: u64,
}

/// Error kind for agent-API request failures (`ReadPane` / `SendText` /
/// `WaitAgentState`). The `emterm mux read/send/wait` CLI exit codes map
/// onto these kinds (see IMPLEMENTATION.md "Conventions"): `invalid_input`
/// → 2, `timeout` → 3, `unknown_pane`/`pane_gone` → 4, `not_mux_pane` → 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApiErrorKind {
    UnknownPane,
    NotMuxPane,
    Timeout,
    PaneGone,
    InvalidInput,
}

/// Structured error response shared by `ReadPane` / `SendText` /
/// `WaitAgentState`, carried as the payload of `MessageType::AgentApiError`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentApiError {
    pub kind: AgentApiErrorKind,
    pub message: String,
}

/// A parsed public-facing pane ID: opaque string form
/// `"{incarnation}-{pane_id}"`, where `incarnation` is a lowercase-hex
/// token minted once at daemon start (never reused across restarts) and
/// `pane_id` is the existing wire `u32`. The daemon is the only minter;
/// clients treat the composed string as opaque and only need
/// [`PublicPaneId::compose`] / [`PublicPaneId::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicPaneId {
    pub incarnation: String,
    pub pane_id: u32,
}

impl PublicPaneId {
    /// Compose the opaque string form from an incarnation token and the
    /// wire pane ID.
    pub fn compose(incarnation: &str, pane_id: u32) -> String {
        format!("{incarnation}-{pane_id}")
    }

    /// Parse a public-facing pane ID string back into its incarnation
    /// token and wire pane ID.
    ///
    /// Never panics: malformed input (empty string, no `-` separator, a
    /// non-lowercase-hex incarnation token, or a pane number that does not
    /// fit in `u32`) yields [`PublicPaneIdError`].
    pub fn parse(id: &str) -> Result<Self, PublicPaneIdError> {
        let (incarnation, pane_id_str) = id
            .rsplit_once('-')
            .ok_or(PublicPaneIdError::MissingSeparator)?;
        if incarnation.is_empty() || !incarnation.chars().all(is_lowercase_hex_digit) {
            return Err(PublicPaneIdError::InvalidIncarnation);
        }
        let pane_id = pane_id_str
            .parse::<u32>()
            .map_err(|_| PublicPaneIdError::InvalidPaneNumber)?;
        Ok(Self {
            incarnation: incarnation.to_string(),
            pane_id,
        })
    }
}

fn is_lowercase_hex_digit(c: char) -> bool {
    c.is_ascii_digit() || ('a'..='f').contains(&c)
}

/// Errors that can occur when parsing a [`PublicPaneId`] string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicPaneIdError {
    /// No `-` separator between incarnation and pane number (also covers
    /// the empty-string input).
    MissingSeparator,
    /// The incarnation token is empty or contains non-lowercase-hex
    /// characters.
    InvalidIncarnation,
    /// The pane-number segment does not parse as a `u32` (non-digits or
    /// overflow).
    InvalidPaneNumber,
}

impl std::fmt::Display for PublicPaneIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeparator => {
                write!(
                    f,
                    "missing '-' separator between incarnation and pane number"
                )
            }
            Self::InvalidIncarnation => write!(f, "incarnation token is not lowercase hex"),
            Self::InvalidPaneNumber => write!(f, "pane number is not a valid u32"),
        }
    }
}

impl std::error::Error for PublicPaneIdError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_type_round_trip() {
        for i in 0x01..=0x1Cu8 {
            if i == 0x11 {
                // 0x11 (SplitPane) was removed -- must return None
                continue;
            }
            let mt = MessageType::from_u8(i).unwrap();
            assert_eq!(mt as u8, i);
        }
        assert!(MessageType::from_u8(0x00).is_none());
        assert!(MessageType::from_u8(0x11).is_none());
        assert_eq!(MessageType::from_u8(0x1B), Some(MessageType::SetVisibility));
        assert_eq!(MessageType::from_u8(0x1C), Some(MessageType::Notify));
        // 0x1D..=0x24 (previously unused) now hold the task0002 agent-status
        // / agent-API additions; see `test_agent_api_message_type_round_trip`
        // for full per-discriminant coverage. The unused-space boundary this
        // assertion pins moves to 0x25.
        assert_eq!(
            MessageType::from_u8(0x1D),
            Some(MessageType::AgentStatusUpdate)
        );
        assert!(MessageType::from_u8(0x25).is_none());
        assert!(MessageType::from_u8(0xff).is_none());
    }

    #[test]
    fn test_notify_message_type() {
        assert_eq!(MessageType::from_u8(0x1C), Some(MessageType::Notify));
        assert_eq!(MessageType::Notify as u8, 0x1C);
    }

    #[test]
    fn test_notify_msg_round_trip() {
        let msg = NotifyMsg {
            message: "build done".to_string(),
        };
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: NotifyMsg = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.message, "build done");
    }

    #[test]
    fn test_notify_msg_via_mux_message() {
        let notify = NotifyMsg {
            message: "ビルド完了 🎉".to_string(),
        };
        let msg = MuxMessage::control(MessageType::Notify, 7, &notify);
        let body = msg.to_frame_body();
        let parsed = MuxMessage::from_frame_body(&body).unwrap();
        assert_eq!(parsed.msg_type, MessageType::Notify);
        assert_eq!(parsed.pane_id, 7);
        let decoded: NotifyMsg = parsed.decode_payload().unwrap();
        assert_eq!(decoded.message, "ビルド完了 🎉");
    }

    #[test]
    fn test_move_window_message_type() {
        assert_eq!(MessageType::from_u8(0x1A), Some(MessageType::MoveWindow));
        assert_eq!(MessageType::MoveWindow as u8, 0x1A);
    }

    #[test]
    fn test_set_visibility_message_type() {
        assert_eq!(MessageType::from_u8(0x1B), Some(MessageType::SetVisibility));
        assert_eq!(MessageType::SetVisibility as u8, 0x1B);
    }

    #[test]
    fn test_set_visibility_payload_round_trip() {
        for visible in [true, false] {
            let payload = SetVisibilityPayload { visible };
            let bytes = payload.to_payload();
            assert_eq!(bytes.len(), 1);
            let decoded = SetVisibilityPayload::from_payload(&bytes).unwrap();
            assert_eq!(decoded.visible, visible);
        }
    }

    #[test]
    fn test_set_visibility_via_mux_message_apc_round_trip() {
        for visible in [true, false] {
            let payload = SetVisibilityPayload { visible };
            let msg = MuxMessage {
                msg_type: MessageType::SetVisibility,
                pane_id: 0,
                payload: payload.to_payload(),
            };
            let apc = msg.to_apc();
            let body = &apc[2..apc.len() - 2];
            let decoded = MuxMessage::from_apc(body).unwrap();
            assert_eq!(decoded.msg_type, MessageType::SetVisibility);
            assert_eq!(decoded.pane_id, 0);
            assert_eq!(decoded.payload.len(), 1);
            let payload_back = SetVisibilityPayload::from_payload(&decoded.payload).unwrap();
            assert_eq!(payload_back.visible, visible);
        }
    }

    #[test]
    fn test_set_visibility_payload_empty_returns_none() {
        assert!(SetVisibilityPayload::from_payload(&[]).is_none());
    }

    #[test]
    fn test_move_window_msg_round_trip() {
        let msg = MoveWindowMsg { target_index: 42 };
        let bytes = bincode::serialize(&msg).unwrap();
        // bincode u32 should be 4 bytes LE
        assert_eq!(bytes.len(), 4);
        let decoded: MoveWindowMsg = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.target_index, 42);
    }

    #[test]
    fn test_move_window_msg_via_mux_message() {
        let move_msg = MoveWindowMsg { target_index: 3 };
        let msg = MuxMessage::control(MessageType::MoveWindow, 99, &move_msg);
        let body = msg.to_frame_body();
        let parsed = MuxMessage::from_frame_body(&body).unwrap();
        assert_eq!(parsed.msg_type, MessageType::MoveWindow);
        assert_eq!(parsed.pane_id, 99);
        let decoded: MoveWindowMsg = parsed.decode_payload().unwrap();
        assert_eq!(decoded.target_index, 3);
    }

    #[test]
    fn test_move_window_msg_zero_index() {
        let msg = MoveWindowMsg { target_index: 0 };
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: MoveWindowMsg = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.target_index, 0);
    }

    #[test]
    fn test_request_status_update_message_type() {
        assert_eq!(
            MessageType::from_u8(0x17),
            Some(MessageType::RequestStatusUpdate)
        );
        assert_eq!(MessageType::RequestStatusUpdate as u8, 0x17);
    }

    #[test]
    fn test_request_pane_snapshot_message_type() {
        assert_eq!(
            MessageType::from_u8(0x19),
            Some(MessageType::RequestPaneSnapshot)
        );
        assert_eq!(MessageType::RequestPaneSnapshot as u8, 0x19);
    }

    #[test]
    fn test_status_update_msg_round_trip() {
        let msg = StatusUpdateMsg {
            left: "hello left".to_string(),
            right: "hello right".to_string(),
        };
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: StatusUpdateMsg = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.left, "hello left");
        assert_eq!(decoded.right, "hello right");
    }

    #[test]
    fn test_status_update_msg_empty_strings() {
        let msg = StatusUpdateMsg {
            left: String::new(),
            right: String::new(),
        };
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: StatusUpdateMsg = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.left, "");
        assert_eq!(decoded.right, "");
    }

    #[test]
    fn test_status_update_msg_unicode() {
        let msg = StatusUpdateMsg {
            left: "ステータス".to_string(),
            right: "右側 🎉".to_string(),
        };
        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: StatusUpdateMsg = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.left, "ステータス");
        assert_eq!(decoded.right, "右側 🎉");
    }

    #[test]
    fn test_status_update_msg_via_mux_message() {
        let status = StatusUpdateMsg {
            left: "left content".to_string(),
            right: "right content".to_string(),
        };
        let msg = MuxMessage::control(MessageType::StatusUpdate, 0, &status);
        let body = msg.to_frame_body();
        let parsed = MuxMessage::from_frame_body(&body).unwrap();
        assert_eq!(parsed.msg_type, MessageType::StatusUpdate);
        let decoded: StatusUpdateMsg = parsed.decode_payload().unwrap();
        assert_eq!(decoded.left, "left content");
        assert_eq!(decoded.right, "right content");
    }

    #[test]
    fn test_pty_output_frame_round_trip() {
        let msg = MuxMessage::pty_output(42, vec![1, 2, 3, 4]);
        let body = msg.to_frame_body();
        let parsed = MuxMessage::from_frame_body(&body).unwrap();
        assert_eq!(parsed.msg_type, MessageType::PtyOutput);
        assert_eq!(parsed.pane_id, 42);
        assert_eq!(parsed.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_control_message_round_trip() {
        let hello = HelloMsg {
            client_type: ClientType::Gui,
            protocol_version: PROTOCOL_VERSION,
        };
        let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
        let body = msg.to_frame_body();
        let parsed = MuxMessage::from_frame_body(&body).unwrap();
        let decoded: HelloMsg = parsed.decode_payload().unwrap();
        assert_eq!(decoded.client_type, ClientType::Gui);
        assert_eq!(decoded.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_welcome_accepted_round_trip() {
        let welcome = WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![SessionInfo {
                id: 1,
                name: "main".to_string(),
                window_count: 2,
                pane_count: 3,
                active_window_index: 0,
                windows: vec![],
            }],
        };
        let msg = MuxMessage::control(MessageType::Welcome, 0, &welcome);
        let decoded: WelcomeMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        match decoded {
            WelcomeMsg::Accepted {
                server_version,
                sessions,
            } => {
                assert_eq!(server_version, 1);
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].name, "main");
            }
            _ => panic!("Expected Accepted"),
        }
    }

    #[test]
    fn test_create_window_payload_both_none() {
        let payload = CreateWindowPayload {
            name: None,
            command: None,
        };
        let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
        let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
        let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
        assert_eq!(decoded.name, None);
        assert_eq!(decoded.command, None);
    }

    #[test]
    fn test_create_window_payload_name_only() {
        let payload = CreateWindowPayload {
            name: Some("editor".to_string()),
            command: None,
        };
        let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
        let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
        let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
        assert_eq!(decoded.name, Some("editor".to_string()));
        assert_eq!(decoded.command, None);
    }

    #[test]
    fn test_create_window_payload_command_only() {
        let payload = CreateWindowPayload {
            name: None,
            command: Some("nvim".to_string()),
        };
        let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
        let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
        let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
        assert_eq!(decoded.name, None);
        assert_eq!(decoded.command, Some("nvim".to_string()));
    }

    #[test]
    fn test_create_window_payload_both_present() {
        let payload = CreateWindowPayload {
            name: Some("editor".to_string()),
            command: Some("nvim".to_string()),
        };
        let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
        let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
        let decoded: CreateWindowPayload = parsed.decode_payload().unwrap();
        assert_eq!(decoded.name, Some("editor".to_string()));
        assert_eq!(decoded.command, Some("nvim".to_string()));
    }

    #[test]
    fn test_create_window_payload_empty_payload_backward_compat() {
        // Empty payload (from GUI) should fail to decode as CreateWindowPayload
        // Handler should use defaults in this case
        let msg = MuxMessage {
            msg_type: MessageType::CreateWindow,
            pane_id: 0,
            payload: vec![],
        };
        let decoded: Option<CreateWindowPayload> = msg.decode_payload();
        // Empty payload cannot be deserialized - handler uses defaults
        assert!(decoded.is_none());
    }

    #[test]
    fn test_create_window_payload_default() {
        let payload = CreateWindowPayload::default();
        assert_eq!(payload.name, None);
        assert_eq!(payload.command, None);
    }

    #[test]
    fn test_from_frame_body_too_short() {
        assert!(MuxMessage::from_frame_body(&[]).is_none());
        assert!(MuxMessage::from_frame_body(&[1, 2, 3, 4]).is_none());
    }

    #[test]
    fn test_from_frame_body_invalid_type() {
        assert!(MuxMessage::from_frame_body(&[0xFF, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn test_empty_payload() {
        let msg = MuxMessage::pty_output(0, vec![]);
        let body = msg.to_frame_body();
        assert_eq!(body.len(), 5); // type + pane_id only
        let parsed = MuxMessage::from_frame_body(&body).unwrap();
        assert!(parsed.payload.is_empty());
    }

    // ---- APC encode/decode tests ----

    #[test]
    fn test_apc_round_trip_pty_output() {
        let msg = MuxMessage::pty_output(42, vec![1, 2, 3, 4]);
        let apc = msg.to_apc();
        // Verify APC format
        assert!(apc.starts_with("\x1b_emterm-mux;"));
        assert!(apc.ends_with("\x1b\\"));
        // Extract payload between delimiters
        let payload = &apc[2..apc.len() - 2]; // strip ESC_ and ESC\
        let decoded = MuxMessage::from_apc(payload).unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 42);
        assert_eq!(decoded.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_apc_round_trip_control_hello() {
        let hello = HelloMsg {
            client_type: ClientType::Gui,
            protocol_version: PROTOCOL_VERSION,
        };
        let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
        let apc = msg.to_apc();
        let payload = &apc[2..apc.len() - 2];
        let decoded = MuxMessage::from_apc(payload).unwrap();
        assert_eq!(decoded.msg_type, MessageType::Hello);
        let hello_decoded: HelloMsg = decoded.decode_payload().unwrap();
        assert_eq!(hello_decoded.client_type, ClientType::Gui);
    }

    #[test]
    fn test_apc_round_trip_all_message_types() {
        for i in 0x01..=0x1Cu8 {
            if i == 0x11 {
                // 0x11 (SplitPane) was removed
                continue;
            }
            let mt = MessageType::from_u8(i).unwrap();
            let msg = MuxMessage {
                msg_type: mt,
                pane_id: i as u32,
                payload: vec![i; 4],
            };
            let apc = msg.to_apc();
            let payload = &apc[2..apc.len() - 2];
            let decoded = MuxMessage::from_apc(payload).unwrap();
            assert_eq!(decoded.msg_type, mt);
            assert_eq!(decoded.pane_id, i as u32);
            assert_eq!(decoded.payload, vec![i; 4]);
        }
    }

    #[test]
    fn test_apc_round_trip_empty_payload() {
        let msg = MuxMessage::pty_output(0, vec![]);
        let apc = msg.to_apc();
        let payload = &apc[2..apc.len() - 2];
        let decoded = MuxMessage::from_apc(payload).unwrap();
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_apc_from_apc_missing_prefix() {
        let err = MuxMessage::from_apc("wrong-prefix;AAAA").unwrap_err();
        assert_eq!(err, ApcDecodeError::MissingPrefix);
    }

    #[test]
    fn test_apc_from_apc_invalid_base64() {
        let err = MuxMessage::from_apc("emterm-mux;!!!invalid!!!").unwrap_err();
        assert_eq!(err, ApcDecodeError::InvalidBase64);
    }

    #[test]
    fn test_apc_from_apc_invalid_frame_body() {
        use base64::Engine;
        // Valid base64 but too short for a frame body (< 5 bytes)
        let encoded = BASE64.encode(&[0x01]);
        let input = format!("emterm-mux;{}", encoded);
        let err = MuxMessage::from_apc(&input).unwrap_err();
        assert_eq!(err, ApcDecodeError::InvalidFrameBody);
    }

    #[test]
    fn test_apc_from_apc_invalid_message_type() {
        use base64::Engine;
        // Valid base64, 5 bytes, but invalid message type 0xFF
        let encoded = BASE64.encode(&[0xFF, 0, 0, 0, 0]);
        let input = format!("emterm-mux;{}", encoded);
        let err = MuxMessage::from_apc(&input).unwrap_err();
        assert_eq!(err, ApcDecodeError::InvalidFrameBody);
    }

    #[test]
    fn test_apc_from_apc_empty_after_prefix() {
        // emterm-mux; with empty base64 => empty bytes => invalid frame body
        let err = MuxMessage::from_apc("emterm-mux;").unwrap_err();
        assert_eq!(err, ApcDecodeError::InvalidFrameBody);
    }

    // ---- OSC encode/decode tests ----

    #[test]
    fn test_osc_round_trip_pty_output() {
        let msg = MuxMessage::pty_output(42, vec![1, 2, 3, 4]);
        let osc = msg.to_osc();
        // Verify OSC format
        assert!(osc.starts_with("\x1b]9999;emterm-mux;"));
        assert!(osc.ends_with("\x1b\\"));
        // Extract the APC-compatible payload (after "9999;")
        // OSC: ESC ] 9999 ; <body> ESC \
        // Strip ESC ] (2 bytes) at start and ESC \ (2 bytes) at end
        let inner = &osc[2..osc.len() - 2]; // "9999;emterm-mux;<base64>"
        let apc_payload = inner.strip_prefix("9999;").unwrap();
        let decoded = MuxMessage::from_apc(apc_payload).unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 42);
        assert_eq!(decoded.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_osc_round_trip_control_hello() {
        let hello = HelloMsg {
            client_type: ClientType::Gui,
            protocol_version: PROTOCOL_VERSION,
        };
        let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
        let osc = msg.to_osc();
        let inner = &osc[2..osc.len() - 2];
        let apc_payload = inner.strip_prefix("9999;").unwrap();
        let decoded = MuxMessage::from_apc(apc_payload).unwrap();
        assert_eq!(decoded.msg_type, MessageType::Hello);
        let hello_decoded: HelloMsg = decoded.decode_payload().unwrap();
        assert_eq!(hello_decoded.client_type, ClientType::Gui);
    }

    #[test]
    fn test_osc_round_trip_empty_payload() {
        let msg = MuxMessage::pty_output(0, vec![]);
        let osc = msg.to_osc();
        let inner = &osc[2..osc.len() - 2];
        let apc_payload = inner.strip_prefix("9999;").unwrap();
        let decoded = MuxMessage::from_apc(apc_payload).unwrap();
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_apc_large_payload() {
        let data = vec![0xAB; 65536];
        let msg = MuxMessage::pty_output(99, data.clone());
        let apc = msg.to_apc();
        let payload = &apc[2..apc.len() - 2];
        let decoded = MuxMessage::from_apc(payload).unwrap();
        assert_eq!(decoded.payload, data);
    }

    // ---- WindowInfo and extended SessionInfo tests ----

    #[test]
    fn test_window_info_serde_roundtrip() {
        let info = WindowInfo {
            id: 1,
            name: "editor".to_string(),
            active_pane_id: 42,
        };
        let bytes = bincode::serialize(&info).unwrap();
        let decoded: WindowInfo = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.name, "editor");
        assert_eq!(decoded.active_pane_id, 42);
    }

    #[test]
    fn test_session_info_with_windows_roundtrip() {
        let info = SessionInfo {
            id: 1,
            name: "main".to_string(),
            window_count: 2,
            pane_count: 3,
            active_window_index: 0,
            windows: vec![
                WindowInfo {
                    id: 1,
                    name: "shell".to_string(),
                    active_pane_id: 10,
                },
                WindowInfo {
                    id: 2,
                    name: "editor".to_string(),
                    active_pane_id: 20,
                },
            ],
        };
        let bytes = bincode::serialize(&info).unwrap();
        let decoded: SessionInfo = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.windows.len(), 2);
        assert_eq!(decoded.windows[0].name, "shell");
        assert_eq!(decoded.windows[0].active_pane_id, 10);
        assert_eq!(decoded.windows[1].name, "editor");
        assert_eq!(decoded.windows[1].active_pane_id, 20);
    }

    #[test]
    fn test_session_info_backward_compat_missing_windows() {
        // Simulate old SessionInfo without windows field (bincode)
        // by serializing a struct that has no windows field
        #[derive(Serialize)]
        struct OldSessionInfo {
            id: u32,
            name: String,
            window_count: u32,
            pane_count: u32,
            active_window_index: u32,
        }
        let old = OldSessionInfo {
            id: 1,
            name: "legacy".to_string(),
            window_count: 1,
            pane_count: 1,
            active_window_index: 0,
        };
        // For bincode, missing trailing field won't deserialize correctly,
        // but serde(default) handles JSON. Test via JSON for backward compat.
        let json = serde_json::to_string(&old).unwrap();
        let decoded: SessionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.name, "legacy");
        assert!(decoded.windows.is_empty());
    }

    // ---- base64 transport inflation metrics (perf regression guard) ----
    //
    // The bridge/ConPTY transport encodes each `MuxMessage` frame body as
    // base64 inside the APC / OSC envelope (`to_apc` / `to_osc`). base64
    // inflates the body by a deterministic ~33% (4 output bytes per 3 input
    // bytes, padded up), on top of a fixed-size envelope. These tests pin the
    // exact byte counts so a future change to the transport encoding (or a
    // regression that double-encodes) is caught, and so the perf work tracking
    // "base64 adds 33%" has a stable, real-time-independent baseline.

    /// base64(STANDARD) output length for `n` input bytes: 4 bytes per 3-byte
    /// group, the final partial group padded to 4. This mirrors what
    /// `BASE64.encode` produces and lets the tests assert the encoded size
    /// without hard-coding magic numbers.
    fn base64_len(n: usize) -> usize {
        n.div_ceil(3) * 4
    }

    #[test]
    fn base64_inflation_to_apc_64kib_payload() {
        // Representative PtyOutput payload: 64 KiB of data.
        let payload_len = 64 * 1024; // 65536
        let msg = MuxMessage::pty_output(7, vec![0xAB; payload_len]);

        let frame_body = msg.to_frame_body();
        // frame body = 1 (type) + 4 (pane_id) + payload
        assert_eq!(frame_body.len(), 5 + payload_len, "frame body layout fixed");

        let apc = msg.to_apc();

        // Fixed envelope: ESC _ (2) + "emterm-mux;" + base64 + ESC \ (2).
        let envelope_overhead = APC_START.len() + APC_PREFIX.len() + APC_ST.len();
        let expected_b64 = base64_len(frame_body.len());
        assert_eq!(
            apc.len(),
            envelope_overhead + expected_b64,
            "to_apc() size = fixed envelope + base64(frame_body)"
        );

        // base64 of a 65541-byte body: ceil(65541/3)*4 = 21847*4 = 87388.
        assert_eq!(
            expected_b64, 87388,
            "base64 length of the 64KiB+5 frame body"
        );

        // The inflation the perf work cares about: encoded-vs-raw-body ratio.
        // 87388 / 65541 ≈ 1.3333 (the canonical base64 +33%). Pin it tight.
        let body = frame_body.len();
        // Express as parts-per-thousand to keep the assertion integer-exact.
        let ratio_permille = expected_b64 * 1000 / body;
        assert_eq!(
            ratio_permille, 1333,
            "base64 inflates the frame body by ~33.3% (1333 permille)"
        );

        // Absolute inflation: encoded body is exactly 21847 bytes larger than
        // the raw body for this payload.
        assert_eq!(expected_b64 - body, 21847, "absolute base64 byte growth");
    }

    #[test]
    fn base64_inflation_to_osc_matches_apc_plus_param() {
        // OSC adds only the "9999;" parameter over the APC envelope; the
        // base64 body is byte-for-byte identical. This pins that the OSC
        // fallback transport does not encode the payload any differently.
        let payload_len = 64 * 1024;
        let msg = MuxMessage::pty_output(3, vec![0xCD; payload_len]);

        let apc = msg.to_apc();
        let osc = msg.to_osc();

        // OSC envelope = ESC ] (2) + "9999" + ";" + "emterm-mux;" + b64 + ESC \.
        // APC envelope = ESC _ (2) + "emterm-mux;" + b64 + ESC \.
        // Both ESC introducers are 2 bytes, so the only delta is "9999;".
        let param_overhead = MUX_OSC_PARAM.to_string().len() + 1; // "9999" + ";"
        assert_eq!(
            osc.len(),
            apc.len() + param_overhead,
            "to_osc() = to_apc() + the OSC \"9999;\" parameter, same base64 body"
        );
        assert_eq!(param_overhead, 5, "OSC param overhead is exactly \"9999;\"");
    }

    #[test]
    fn base64_inflation_ratio_is_payload_size_independent() {
        // The ~33% inflation holds across payload sizes (only the fixed
        // envelope changes the headline ratio for tiny payloads). Verify the
        // base64 body ratio converges to 4/3 as the payload grows, so the
        // perf model "base64 = +33%" is sound for the bulk-output case the
        // regression targets.
        for &payload_len in &[4 * 1024usize, 16 * 1024, 256 * 1024] {
            let msg = MuxMessage::pty_output(1, vec![0u8; payload_len]);
            let body = msg.to_frame_body().len();
            let encoded_body = base64_len(body);
            // 1333 permille = +33.3%. Large payloads stay within 1 permille.
            let ratio_permille = encoded_body * 1000 / body;
            assert!(
                (1333..=1334).contains(&ratio_permille),
                "payload {payload_len}: base64 body ratio {ratio_permille} permille not ~1333"
            );
        }
    }

    // ---- to_plaintext (Windows ConPTY input transport) ----

    #[test]
    fn to_plaintext_has_emux_prefix_and_cr_terminator() {
        let msg = MuxMessage::pty_output(7, b"hi".to_vec());
        let pt = msg.to_plaintext();
        assert!(pt.starts_with("EMUX;"), "got {pt:?}");
        assert!(pt.ends_with('\r'), "got {pt:?}");
        // No APC / OSC escapes in the body — ConPTY input strips those.
        assert!(
            !pt.contains('\x1b'),
            "plaintext envelope must be escape-free, got {pt:?}"
        );
        // CR is VK_RETURN through ConPTY's WIN32_INPUT_MODE; LF is NOT a
        // standard key and gets dropped on the host→bridge path, so the
        // terminator must be CR. This pins that regression.
        assert!(
            !pt.contains('\n'),
            "plaintext envelope must not carry LF (drops under ConPTY WIN32_INPUT_MODE), got {pt:?}"
        );
    }

    #[test]
    fn to_plaintext_round_trips_with_bridge_parser_shape() {
        // The bridge's StdinApcParser strips the EMUX; prefix and \r
        // terminator, then prepends APC_PREFIX before calling from_apc.
        // Mirror that shape here so a wire-format drift between encoder and
        // parser fails this test.
        let payload = StatusUpdateMsg {
            left: "left 🦀".to_string(),
            right: "right ✨".to_string(),
        };
        let msg = MuxMessage::control(MessageType::StatusUpdate, 11, &payload);

        let pt = msg.to_plaintext();
        let body = pt
            .strip_prefix("EMUX;")
            .and_then(|s| s.strip_suffix('\r'))
            .expect("plaintext envelope");
        let with_apc_prefix = format!("{}{}", APC_PREFIX, body);
        let decoded = MuxMessage::from_apc(&with_apc_prefix).expect("decoded");

        assert_eq!(decoded.msg_type, MessageType::StatusUpdate);
        assert_eq!(decoded.pane_id, 11);
        let back: StatusUpdateMsg = decoded.decode_payload().unwrap();
        assert_eq!(back.left, "left 🦀");
        assert_eq!(back.right, "right ✨");
    }

    #[test]
    fn to_plaintext_body_matches_to_apc_body() {
        // Both transports base64-encode the SAME frame body; only the
        // envelope differs. This pins that to_plaintext does not double-
        // wrap or otherwise change the protocol payload.
        let msg = MuxMessage::pty_output(3, vec![0xAB; 1024]);

        let apc = msg.to_apc();
        let pt = msg.to_plaintext();

        let apc_body = apc
            .strip_prefix("\x1b_emterm-mux;")
            .and_then(|s| s.strip_suffix("\x1b\\"))
            .unwrap();
        let pt_body = pt
            .strip_prefix("EMUX;")
            .and_then(|s| s.strip_suffix('\r'))
            .unwrap();
        assert_eq!(apc_body, pt_body, "base64 frame body must be identical");
    }

    #[test]
    fn test_welcome_with_windows_roundtrip() {
        let welcome = WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![SessionInfo {
                id: 1,
                name: "main".to_string(),
                window_count: 1,
                pane_count: 1,
                active_window_index: 0,
                windows: vec![WindowInfo {
                    id: 1,
                    name: "shell".to_string(),
                    active_pane_id: 5,
                }],
            }],
        };
        let msg = MuxMessage::control(MessageType::Welcome, 0, &welcome);
        let decoded: WelcomeMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        match decoded {
            WelcomeMsg::Accepted { sessions, .. } => {
                assert_eq!(sessions[0].windows.len(), 1);
                assert_eq!(sessions[0].windows[0].active_pane_id, 5);
            }
            _ => panic!("Expected Accepted"),
        }
    }

    // ---- agent-status / agent-API message additions (task0002) ----

    /// AC-3: `from_u8` maps every new discriminant, and the space right
    /// after the extended range is still unmapped.
    #[test]
    fn test_agent_api_message_type_round_trip() {
        for i in 0x1Du8..=0x24u8 {
            let mt = MessageType::from_u8(i).unwrap();
            assert_eq!(mt as u8, i);
        }
        assert_eq!(
            MessageType::from_u8(0x1D),
            Some(MessageType::AgentStatusUpdate)
        );
        assert_eq!(MessageType::from_u8(0x1E), Some(MessageType::ReadPane));
        assert_eq!(
            MessageType::from_u8(0x1F),
            Some(MessageType::ReadPaneResult)
        );
        assert_eq!(MessageType::from_u8(0x20), Some(MessageType::SendText));
        assert_eq!(
            MessageType::from_u8(0x21),
            Some(MessageType::SendTextResult)
        );
        assert_eq!(
            MessageType::from_u8(0x22),
            Some(MessageType::WaitAgentState)
        );
        assert_eq!(
            MessageType::from_u8(0x23),
            Some(MessageType::WaitAgentStateResult)
        );
        assert_eq!(MessageType::from_u8(0x24), Some(MessageType::AgentApiError));
        assert!(MessageType::from_u8(0x25).is_none());
    }

    /// AC-1 / AC-3: APC round trip for every new discriminant, mirroring
    /// `test_apc_round_trip_all_message_types` for the pre-existing range.
    #[test]
    fn test_apc_round_trip_agent_api_message_types() {
        for i in 0x1Du8..=0x24u8 {
            let mt = MessageType::from_u8(i).unwrap();
            let msg = MuxMessage {
                msg_type: mt,
                pane_id: i as u32,
                payload: vec![i; 4],
            };
            let apc = msg.to_apc();
            let payload = &apc[2..apc.len() - 2];
            let decoded = MuxMessage::from_apc(payload).unwrap();
            assert_eq!(decoded.msg_type, mt);
            assert_eq!(decoded.pane_id, i as u32);
            assert_eq!(decoded.payload, vec![i; 4]);
        }
    }

    /// AC-1: `AgentStatusUpdate` round-trips with a `Set`-like payload
    /// (state + name present, not replay-derived).
    #[test]
    fn test_agent_status_update_msg_round_trip_set() {
        let update = AgentStatusUpdateMsg {
            pane_id: 7,
            public_pane_id: "ab12cd34-7".to_string(),
            state: Some(AgentState::Working),
            name: Some("build".to_string()),
            revision: 3,
            replay_derived: false,
        };
        let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 7, &update);
        let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
        assert_eq!(parsed.msg_type, MessageType::AgentStatusUpdate);
        assert_eq!(parsed.pane_id, 7);
        let decoded: AgentStatusUpdateMsg = parsed.decode_payload().unwrap();
        assert_eq!(decoded.pane_id, 7);
        assert_eq!(decoded.public_pane_id, "ab12cd34-7");
        assert_eq!(decoded.state, Some(AgentState::Working));
        assert_eq!(decoded.name, Some("build".to_string()));
        assert_eq!(decoded.revision, 3);
        assert!(!decoded.replay_derived);
    }

    /// AC-1: `AgentStatusUpdate` round-trips with a `Clear`-like payload
    /// (state + name absent) and `replay_derived: true`.
    #[test]
    fn test_agent_status_update_msg_round_trip_clear_replay_derived() {
        let update = AgentStatusUpdateMsg {
            pane_id: 12,
            public_pane_id: "ab12cd34-12".to_string(),
            state: None,
            name: None,
            revision: 9,
            replay_derived: true,
        };
        let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 12, &update);
        let decoded: AgentStatusUpdateMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.state, None);
        assert_eq!(decoded.name, None);
        assert_eq!(decoded.revision, 9);
        assert!(decoded.replay_derived);
    }

    /// AC-1: `ReadPane` request / `ReadPaneResult` response round-trip.
    #[test]
    fn test_read_pane_request_and_result_round_trip() {
        let req = ReadPaneMsg {
            public_pane_id: "ab12cd34-3".to_string(),
            lines: 200,
        };
        let req_msg = MuxMessage::control(MessageType::ReadPane, 3, &req);
        let decoded_req: ReadPaneMsg = MuxMessage::from_frame_body(&req_msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded_req.public_pane_id, "ab12cd34-3");
        assert_eq!(decoded_req.lines, 200);

        let result = ReadPaneResultMsg {
            text: "line1\nline2\n🎉".to_string(),
        };
        let result_msg = MuxMessage::control(MessageType::ReadPaneResult, 3, &result);
        let decoded_result: ReadPaneResultMsg =
            MuxMessage::from_frame_body(&result_msg.to_frame_body())
                .unwrap()
                .decode_payload()
                .unwrap();
        assert_eq!(decoded_result.text, "line1\nline2\n🎉");
    }

    /// AC-1: `SendText` request / `SendTextResult` response round-trip.
    #[test]
    fn test_send_text_request_and_result_round_trip() {
        let req = SendTextMsg {
            public_pane_id: "ab12cd34-5".to_string(),
            bytes: b"echo hi\n".to_vec(),
        };
        let req_msg = MuxMessage::control(MessageType::SendText, 5, &req);
        let decoded_req: SendTextMsg = MuxMessage::from_frame_body(&req_msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded_req.public_pane_id, "ab12cd34-5");
        assert_eq!(decoded_req.bytes, b"echo hi\n".to_vec());

        let result = SendTextResultMsg {
            revision_watermark: 42,
        };
        let result_msg = MuxMessage::control(MessageType::SendTextResult, 5, &result);
        let decoded_result: SendTextResultMsg =
            MuxMessage::from_frame_body(&result_msg.to_frame_body())
                .unwrap()
                .decode_payload()
                .unwrap();
        assert_eq!(decoded_result.revision_watermark, 42);
    }

    /// AC-1: `WaitAgentState` request / `WaitAgentStateResult` response
    /// round-trip, with `after_revision` present.
    #[test]
    fn test_wait_agent_state_request_and_result_round_trip() {
        let req = WaitAgentStateMsg {
            public_pane_id: "ab12cd34-9".to_string(),
            states: vec![AgentState::Blocked, AgentState::Done],
            timeout_ms: 5000,
            after_revision: Some(10),
        };
        let req_msg = MuxMessage::control(MessageType::WaitAgentState, 9, &req);
        let decoded_req: WaitAgentStateMsg = MuxMessage::from_frame_body(&req_msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded_req.public_pane_id, "ab12cd34-9");
        assert_eq!(
            decoded_req.states,
            vec![AgentState::Blocked, AgentState::Done]
        );
        assert_eq!(decoded_req.timeout_ms, 5000);
        assert_eq!(decoded_req.after_revision, Some(10));

        let result = WaitAgentStateResultMsg {
            state: AgentState::Done,
            revision: 11,
        };
        let result_msg = MuxMessage::control(MessageType::WaitAgentStateResult, 9, &result);
        let decoded_result: WaitAgentStateResultMsg =
            MuxMessage::from_frame_body(&result_msg.to_frame_body())
                .unwrap()
                .decode_payload()
                .unwrap();
        assert_eq!(decoded_result.state, AgentState::Done);
        assert_eq!(decoded_result.revision, 11);
    }

    /// AC-1: `WaitAgentState` request round-trips with `after_revision: None`.
    #[test]
    fn test_wait_agent_state_request_round_trip_no_after_revision() {
        let req = WaitAgentStateMsg {
            public_pane_id: "ab12cd34-1".to_string(),
            states: vec![AgentState::Idle],
            timeout_ms: 0,
            after_revision: None,
        };
        let msg = MuxMessage::control(MessageType::WaitAgentState, 1, &req);
        let decoded: WaitAgentStateMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.after_revision, None);
        assert_eq!(decoded.timeout_ms, 0);
    }

    /// AC-1: `AgentApiError` round-trips for every error kind.
    #[test]
    fn test_agent_api_error_round_trip_all_kinds() {
        let kinds = [
            AgentApiErrorKind::UnknownPane,
            AgentApiErrorKind::NotMuxPane,
            AgentApiErrorKind::Timeout,
            AgentApiErrorKind::PaneGone,
            AgentApiErrorKind::InvalidInput,
        ];
        for kind in kinds {
            let err = AgentApiError {
                kind,
                message: format!("error: {kind:?}"),
            };
            let msg = MuxMessage::control(MessageType::AgentApiError, 0, &err);
            let decoded: AgentApiError = MuxMessage::from_frame_body(&msg.to_frame_body())
                .unwrap()
                .decode_payload()
                .unwrap();
            assert_eq!(decoded.kind, kind);
            assert_eq!(decoded.message, format!("error: {kind:?}"));
        }
    }

    /// AC-1: `AgentApiErrorKind` serializes to the exact lowercase-snake
    /// wire strings the CLI exit-code mapping depends on.
    #[test]
    fn test_agent_api_error_kind_wire_strings() {
        let cases = [
            (AgentApiErrorKind::UnknownPane, "\"unknown_pane\""),
            (AgentApiErrorKind::NotMuxPane, "\"not_mux_pane\""),
            (AgentApiErrorKind::Timeout, "\"timeout\""),
            (AgentApiErrorKind::PaneGone, "\"pane_gone\""),
            (AgentApiErrorKind::InvalidInput, "\"invalid_input\""),
        ];
        for (kind, expected_json) in cases {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected_json);
        }
    }

    /// AC-1: `AgentState` serializes to the exact lowercase wire strings
    /// the core `agent_status` module's mirror contract depends on.
    #[test]
    fn test_agent_state_wire_strings() {
        let cases = [
            (AgentState::Idle, "\"idle\""),
            (AgentState::Working, "\"working\""),
            (AgentState::Blocked, "\"blocked\""),
            (AgentState::Done, "\"done\""),
        ];
        for (state, expected_json) in cases {
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, expected_json);
        }
    }

    // ---- public pane ID helpers (task0002) ----

    /// AC-4: compose → parse round-trips.
    #[test]
    fn test_public_pane_id_compose_parse_round_trip() {
        let composed = PublicPaneId::compose("ab12cd34", 7);
        assert_eq!(composed, "ab12cd34-7");
        let parsed = PublicPaneId::parse(&composed).unwrap();
        assert_eq!(
            parsed,
            PublicPaneId {
                incarnation: "ab12cd34".to_string(),
                pane_id: 7,
            }
        );
    }

    #[test]
    fn test_public_pane_id_compose_parse_round_trip_pane_zero() {
        let composed = PublicPaneId::compose("0f", 0);
        let parsed = PublicPaneId::parse(&composed).unwrap();
        assert_eq!(parsed.incarnation, "0f");
        assert_eq!(parsed.pane_id, 0);
    }

    /// AC-4: parsing an empty string returns an error, never a panic.
    #[test]
    fn test_public_pane_id_parse_rejects_empty() {
        assert!(PublicPaneId::parse("").is_err());
    }

    /// AC-4: parsing a string with no `-` separator returns an error.
    #[test]
    fn test_public_pane_id_parse_rejects_missing_separator() {
        let err = PublicPaneId::parse("ab12cd347").unwrap_err();
        assert_eq!(err, PublicPaneIdError::MissingSeparator);
    }

    /// AC-4: parsing a string whose incarnation segment is not lowercase
    /// hex returns an error.
    #[test]
    fn test_public_pane_id_parse_rejects_non_hex_incarnation() {
        let err = PublicPaneId::parse("AB12CD34-7").unwrap_err();
        assert_eq!(err, PublicPaneIdError::InvalidIncarnation);

        let err = PublicPaneId::parse("not-hex-zone-7").unwrap_err();
        assert_eq!(err, PublicPaneIdError::InvalidIncarnation);

        let err = PublicPaneId::parse("-7").unwrap_err();
        assert_eq!(err, PublicPaneIdError::InvalidIncarnation);
    }

    /// AC-4: parsing a pane-number segment that overflows `u32` returns an
    /// error.
    #[test]
    fn test_public_pane_id_parse_rejects_pane_number_overflow() {
        let err = PublicPaneId::parse("ab12cd34-4294967296").unwrap_err();
        assert_eq!(err, PublicPaneIdError::InvalidPaneNumber);
    }

    /// AC-4: parsing a pane-number segment that is not numeric at all
    /// returns an error.
    #[test]
    fn test_public_pane_id_parse_rejects_non_numeric_pane_number() {
        let err = PublicPaneId::parse("ab12cd34-abc").unwrap_err();
        assert_eq!(err, PublicPaneIdError::InvalidPaneNumber);
    }

    // ---- PROTOCOL_VERSION bump (task0002) ----

    /// AC-5: `PROTOCOL_VERSION` is bumped exactly once for this task, to 2.
    #[test]
    fn test_protocol_version_bumped_for_agent_api_additions() {
        assert_eq!(PROTOCOL_VERSION, 2);
    }

    // ---- task0010 rework: safe PROTOCOL_VERSION upgrade path (strategy B) ----

    /// AC-1: the adjacent-version constant tracks `PROTOCOL_VERSION - 1`
    /// exactly, so a future bump keeps the recovery retry one version back.
    #[test]
    fn test_previous_protocol_version_is_adjacent() {
        assert_eq!(PREVIOUS_PROTOCOL_VERSION, PROTOCOL_VERSION - 1);
        assert_eq!(PREVIOUS_PROTOCOL_VERSION, 1);
    }

    /// AC-1: parses the exact reason text the daemon's version-mismatch
    /// path produces.
    #[test]
    fn test_parse_rejected_server_version_matches_daemon_format() {
        let reason = format!(
            "Protocol version mismatch: client={}, server={}",
            PROTOCOL_VERSION, PREVIOUS_PROTOCOL_VERSION
        );
        assert_eq!(
            parse_rejected_server_version(&reason),
            Some(PREVIOUS_PROTOCOL_VERSION)
        );
    }

    /// AC-3: a rejection for any other reason never gets misread as a
    /// version number — no panic, just `None`.
    #[test]
    fn test_parse_rejected_server_version_returns_none_for_unrelated_reason() {
        assert_eq!(parse_rejected_server_version("Connection refused"), None);
        assert_eq!(parse_rejected_server_version(""), None);
        assert_eq!(parse_rejected_server_version("server=not-a-number"), None);
        assert_eq!(parse_rejected_server_version("server="), None);
    }

    /// AC-1: only the digits immediately after `server=` are consumed, so
    /// trailing text in a future reason format doesn't corrupt the parse.
    #[test]
    fn test_parse_rejected_server_version_stops_at_non_digit() {
        assert_eq!(
            parse_rejected_server_version("client=2, server=1 (extra info)"),
            Some(1)
        );
    }
}
