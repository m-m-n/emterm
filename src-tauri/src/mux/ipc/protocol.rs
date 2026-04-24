//! IPC message types and frame format for mux daemon communication.
//!
//! Frame format: [length: u32][type: u8][pane_id: u32][payload: variable]
//! - length: remaining bytes after the length field (= 5 + payload_len)
//! - PTY data uses raw bytes payload
//! - Control messages use bincode-serialized payload

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

/// Protocol version for handshake compatibility check.
pub const PROTOCOL_VERSION: u32 = 1;

/// APC prefix for identifying emterm mux APC sequences.
pub const APC_PREFIX: &str = "emterm-mux;";

/// APC introducer: ESC _
const APC_START: &str = "\x1b_";

/// APC string terminator: ESC \
const APC_ST: &str = "\x1b\\";

/// OSC parameter for emterm mux messages.
pub const MUX_OSC_PARAM: u16 = 9999;

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
        for i in 0x01..=0x1Au8 {
            if i == 0x11 {
                // 0x11 (SplitPane) was removed -- must return None
                continue;
            }
            let mt = MessageType::from_u8(i).unwrap();
            assert_eq!(mt as u8, i);
        }
        assert!(MessageType::from_u8(0x00).is_none());
        assert!(MessageType::from_u8(0x11).is_none());
        assert!(MessageType::from_u8(0x1b).is_none());
    }

    #[test]
    fn test_move_window_message_type() {
        assert_eq!(MessageType::from_u8(0x1A), Some(MessageType::MoveWindow));
        assert_eq!(MessageType::MoveWindow as u8, 0x1A);
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
        for i in 0x01..=0x1Au8 {
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
}
