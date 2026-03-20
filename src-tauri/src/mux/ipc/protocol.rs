//! IPC message types and frame format for mux daemon communication.
//!
//! Frame format: [length: u32][type: u8][pane_id: u32][payload: variable]
//! - length: remaining bytes after the length field (= 5 + payload_len)
//! - PTY data uses raw bytes payload
//! - Control messages use bincode-serialized payload

use serde::{Deserialize, Serialize};

/// Protocol version for handshake compatibility check.
pub const PROTOCOL_VERSION: u32 = 1;

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

/// Session info returned in Welcome message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub window_count: u32,
    pub pane_count: u32,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_type_round_trip() {
        for i in 0x01..=0x10u8 {
            let mt = MessageType::from_u8(i).unwrap();
            assert_eq!(mt as u8, i);
        }
        assert!(MessageType::from_u8(0x00).is_none());
        assert!(MessageType::from_u8(0x11).is_none());
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
}
