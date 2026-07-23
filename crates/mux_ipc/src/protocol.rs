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
/// Bumped 1 -> 2 for the mux-agent-status-api feature's new message types
/// (`ReadPane` / `SendText` / `WaitAgentState` and their result / error
/// payloads, task0004's provisional addition pending task0002's canonical
/// landing — see IMPLEMENTATION.md "mux_ipc protocol additions"). Existing
/// message encodings, `StatusUpdate`, and `Snapshot` payload bytes are
/// unchanged; a client/daemon pairing with mismatched
/// `PROTOCOL_VERSION` fails the handshake cleanly via the existing
/// `WelcomeMsg::Rejected` path (see `mux::ipc::connection::handle_connection`)
/// rather than misparsing.
pub const PROTOCOL_VERSION: u32 = 2;

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
    // mux-agent-status-api additions (task0004 provisional; see
    // IMPLEMENTATION.md "mux_ipc protocol additions" / "Public pane ID
    // format"). Requests are CLI -> daemon; results/error are daemon ->
    // CLI. `AgentStatusUpdate` (daemon -> GUI, unsolicited) is reserved
    // for task0003/task0005 and intentionally not defined here.
    /// Request: read the tail N rendered rows of a mux pane
    /// (`ReadPaneMsg` -> `ReadPaneResultMsg` | `AgentApiErrorMsg`).
    ReadPane = 0x1D,
    /// Request: write bytes to a mux pane's PTY
    /// (`SendTextMsg` -> `SendTextResultMsg` | `AgentApiErrorMsg`).
    SendText = 0x1E,
    /// Request: block until a mux pane's agent state enters a given set
    /// (`WaitAgentStateMsg` -> `WaitAgentStateResultMsg` | `AgentApiErrorMsg`).
    WaitAgentState = 0x1F,
    /// Response payload for `ReadPane`.
    ReadPaneResult = 0x20,
    /// Response payload for `SendText`.
    SendTextResult = 0x21,
    /// Response payload for `WaitAgentState`.
    WaitAgentStateResult = 0x22,
    /// Shared error response for `ReadPane` / `SendText` / `WaitAgentState`.
    AgentApiError = 0x23,
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
            0x1D => Some(Self::ReadPane),
            0x1E => Some(Self::SendText),
            0x1F => Some(Self::WaitAgentState),
            0x20 => Some(Self::ReadPaneResult),
            0x21 => Some(Self::SendTextResult),
            0x22 => Some(Self::WaitAgentStateResult),
            0x23 => Some(Self::AgentApiError),
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

// ---- mux-agent-status-api: agent-facing API messages (task0004 provisional) ----
//
// These types implement the wire contract pinned in IMPLEMENTATION.md
// ("mux_ipc protocol additions" / "Public pane ID format" / "Revision
// semantics"). They are declared here — ahead of task0002, which formally
// owns this file — because task0004 (daemon handlers + `emterm mux`
// CLI) cannot compile without them; task0002 landing its own version is
// expected to produce a merge conflict resolved via parent-side adoption
// (re-implementing task0004's handler code against task0002's canonical
// shapes, which are pinned to be identical to these).

/// Agent state reported via `OSC 777;emterm;agent-status`. Local mirror of
/// the core module's enum (`src-tauri/src/agent_status.rs`, task0001):
/// `mux_ipc` must not depend on the binary crate, so this crate hosts the
/// plain wire-representation type and the core module is expected to reuse
/// it. The string wire values (`idle|working|blocked|done`) are the FR1
/// contract; this enum's variant order is NOT the wire encoding (bincode
/// encodes by discriminant, never observed off-process).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    Idle,
    Working,
    Blocked,
    Done,
}

/// Request: read the tail `lines` rendered rows of a mux pane (current
/// screen + scrollback tail), ANSI-stripped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPaneMsg {
    pub public_pane_id: String,
    pub lines: u32,
}

/// Response to `ReadPaneMsg`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPaneResultMsg {
    pub text: String,
}

/// Request: write `bytes` verbatim to a mux pane's PTY (no implicit Enter,
/// no key interpretation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextMsg {
    pub public_pane_id: String,
    pub bytes: Vec<u8>,
}

/// Response to `SendTextMsg`: the pane's revision as observed immediately
/// before the successful write (the watermark).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextResultMsg {
    pub revision_watermark: u64,
}

/// Request: block (server-side) until the pane's agent state enters
/// `states`, optionally requiring `revision > after_revision`
/// (send-then-wait linearization). Level-triggered: an already-qualifying
/// state at request time resolves immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentStateMsg {
    pub public_pane_id: String,
    pub states: Vec<AgentState>,
    pub timeout_ms: u64,
    pub after_revision: Option<u64>,
}

/// Response to `WaitAgentStateMsg` on success.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentStateResultMsg {
    pub state: AgentState,
    pub revision: u64,
}

/// Error kinds shared by `ReadPane` / `SendText` / `WaitAgentState`.
///
/// `NotMuxPane` is the CLI-facing name reserved for targets outside the
/// daemon's pane set (e.g. plain tabs). Per task0004's design decision the
/// daemon currently resolves that case identically to `UnknownPane` on the
/// wire (both wire-identical per the shared error contract); the CLI's
/// exit-code mapping still keys off this variant so a future daemon
/// revision can emit it distinctly without a CLI change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentApiErrorKind {
    UnknownPane,
    NotMuxPane,
    Timeout,
    PaneGone,
    InvalidInput,
}

/// Error response for `ReadPane` / `SendText` / `WaitAgentState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentApiErrorMsg {
    pub kind: AgentApiErrorKind,
    pub message: String,
}

/// Compose a public (API-facing, opaque) pane ID from a daemon incarnation
/// token and the internal wire pane ID. See IMPLEMENTATION.md "Public pane
/// ID format": `"{incarnation}-{pane_id}"`. Pure — the incarnation token
/// itself is minted daemon-side (task0003/task0004), not by this crate.
pub fn compose_public_pane_id(incarnation: &str, pane_id: u32) -> String {
    format!("{incarnation}-{pane_id}")
}

/// Parse a public pane ID back into its `(incarnation, pane_id)` parts.
///
/// Returns `None` — never panics — for any malformed input: missing
/// separator, empty/non-lowercase-hex incarnation, or a pane-id segment
/// that fails to parse as `u32` (non-numeric or overflow). Splits on the
/// LAST `-` so a hypothetical future incarnation scheme containing `-` still
/// resolves correctly against the purely-numeric pane-id suffix.
pub fn parse_public_pane_id(s: &str) -> Option<(String, u32)> {
    let (incarnation, pane_id_str) = s.rsplit_once('-')?;
    if incarnation.is_empty()
        || !incarnation
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() && b.is_ascii_hexdigit())
    {
        return None;
    }
    let pane_id = pane_id_str.parse::<u32>().ok()?;
    Some((incarnation.to_string(), pane_id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_type_round_trip() {
        for i in 0x01..=0x23u8 {
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
        assert!(MessageType::from_u8(0x24).is_none());
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
        for i in 0x01..=0x23u8 {
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

    // ---- mux-agent-status-api: agent API message round-trips (task0004) ----

    #[test]
    fn test_read_pane_msg_via_mux_message() {
        let req = ReadPaneMsg {
            public_pane_id: "abc123-7".to_string(),
            lines: 100,
        };
        let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
        let parsed = MuxMessage::from_frame_body(&msg.to_frame_body()).unwrap();
        assert_eq!(parsed.msg_type, MessageType::ReadPane);
        let decoded: ReadPaneMsg = parsed.decode_payload().unwrap();
        assert_eq!(decoded.public_pane_id, "abc123-7");
        assert_eq!(decoded.lines, 100);
    }

    #[test]
    fn test_read_pane_result_msg_round_trip() {
        let result = ReadPaneResultMsg {
            text: "line1\nline2\n日本語".to_string(),
        };
        let msg = MuxMessage::control(MessageType::ReadPaneResult, 0, &result);
        let decoded: ReadPaneResultMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.text, "line1\nline2\n日本語");
    }

    #[test]
    fn test_send_text_msg_round_trip_with_bytes() {
        let req = SendTextMsg {
            public_pane_id: "abc123-7".to_string(),
            bytes: b"hello\n".to_vec(),
        };
        let msg = MuxMessage::control(MessageType::SendText, 0, &req);
        let decoded: SendTextMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.public_pane_id, "abc123-7");
        assert_eq!(decoded.bytes, b"hello\n");
    }

    #[test]
    fn test_send_text_result_msg_round_trip() {
        let result = SendTextResultMsg {
            revision_watermark: 42,
        };
        let msg = MuxMessage::control(MessageType::SendTextResult, 0, &result);
        let decoded: SendTextResultMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.revision_watermark, 42);
    }

    #[test]
    fn test_wait_agent_state_msg_round_trip() {
        let req = WaitAgentStateMsg {
            public_pane_id: "abc123-7".to_string(),
            states: vec![AgentState::Blocked, AgentState::Done],
            timeout_ms: 30_000,
            after_revision: Some(5),
        };
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        let decoded: WaitAgentStateMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.public_pane_id, "abc123-7");
        assert_eq!(decoded.states, vec![AgentState::Blocked, AgentState::Done]);
        assert_eq!(decoded.timeout_ms, 30_000);
        assert_eq!(decoded.after_revision, Some(5));
    }

    #[test]
    fn test_wait_agent_state_msg_no_after_revision() {
        let req = WaitAgentStateMsg {
            public_pane_id: "x-1".to_string(),
            states: vec![AgentState::Idle],
            timeout_ms: 0,
            after_revision: None,
        };
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        let decoded: WaitAgentStateMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.after_revision, None);
    }

    #[test]
    fn test_wait_agent_state_result_msg_round_trip() {
        let result = WaitAgentStateResultMsg {
            state: AgentState::Working,
            revision: 3,
        };
        let msg = MuxMessage::control(MessageType::WaitAgentStateResult, 0, &result);
        let decoded: WaitAgentStateResultMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
            .unwrap()
            .decode_payload()
            .unwrap();
        assert_eq!(decoded.state, AgentState::Working);
        assert_eq!(decoded.revision, 3);
    }

    #[test]
    fn test_agent_api_error_msg_round_trip_all_kinds() {
        for kind in [
            AgentApiErrorKind::UnknownPane,
            AgentApiErrorKind::NotMuxPane,
            AgentApiErrorKind::Timeout,
            AgentApiErrorKind::PaneGone,
            AgentApiErrorKind::InvalidInput,
        ] {
            let err = AgentApiErrorMsg {
                kind,
                message: "boom".to_string(),
            };
            let msg = MuxMessage::control(MessageType::AgentApiError, 0, &err);
            let decoded: AgentApiErrorMsg = MuxMessage::from_frame_body(&msg.to_frame_body())
                .unwrap()
                .decode_payload()
                .unwrap();
            assert_eq!(decoded.kind, kind);
            assert_eq!(decoded.message, "boom");
        }
    }

    #[test]
    fn test_read_send_wait_message_types_discriminants() {
        assert_eq!(MessageType::ReadPane as u8, 0x1D);
        assert_eq!(MessageType::SendText as u8, 0x1E);
        assert_eq!(MessageType::WaitAgentState as u8, 0x1F);
        assert_eq!(MessageType::ReadPaneResult as u8, 0x20);
        assert_eq!(MessageType::SendTextResult as u8, 0x21);
        assert_eq!(MessageType::WaitAgentStateResult as u8, 0x22);
        assert_eq!(MessageType::AgentApiError as u8, 0x23);
    }

    // ---- public pane ID compose/parse (task0004 provisional, IMPLEMENTATION.md
    // "Public pane ID format") ----

    #[test]
    fn public_pane_id_compose_parse_round_trip() {
        let composed = compose_public_pane_id("a1b2c3d4", 42);
        assert_eq!(composed, "a1b2c3d4-42");
        let (incarnation, pane_id) = parse_public_pane_id(&composed).unwrap();
        assert_eq!(incarnation, "a1b2c3d4");
        assert_eq!(pane_id, 42);
    }

    #[test]
    fn public_pane_id_compose_parse_round_trip_pane_id_zero() {
        let composed = compose_public_pane_id("00", 0);
        let (incarnation, pane_id) = parse_public_pane_id(&composed).unwrap();
        assert_eq!(incarnation, "00");
        assert_eq!(pane_id, 0);
    }

    #[test]
    fn public_pane_id_parse_rejects_empty() {
        assert!(parse_public_pane_id("").is_none());
    }

    #[test]
    fn public_pane_id_parse_rejects_missing_separator() {
        assert!(parse_public_pane_id("nodash").is_none());
    }

    #[test]
    fn public_pane_id_parse_rejects_non_hex_incarnation() {
        assert!(parse_public_pane_id("not-hex-42").is_none());
        assert!(parse_public_pane_id("zz-1").is_none());
    }

    #[test]
    fn public_pane_id_parse_rejects_uppercase_incarnation() {
        // "lowercase-hex token" per IMPLEMENTATION.md: uppercase hex digits
        // are rejected even though they are valid hex.
        assert!(parse_public_pane_id("ABCD-1").is_none());
    }

    #[test]
    fn public_pane_id_parse_rejects_pane_id_overflow() {
        // u32::MAX + 1
        assert!(parse_public_pane_id("abcd-4294967296").is_none());
    }

    #[test]
    fn public_pane_id_parse_rejects_non_numeric_pane_id() {
        assert!(parse_public_pane_id("abcd-notanumber").is_none());
    }

    #[test]
    fn public_pane_id_parse_rejects_empty_incarnation() {
        assert!(parse_public_pane_id("-42").is_none());
    }
}
