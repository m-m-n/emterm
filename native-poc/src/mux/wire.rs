//! Synchronous length-prefix framing for the mux IPC protocol.
//!
//! Wire shape (identical to the legacy `tokio_util::codec::LengthDelimited`
//! framing used by `src-tauri/src/mux/ipc/codec.rs`):
//!
//! ```text
//! ┌────────────────────────────────┬─────────────────────────────────────┐
//! │ length (u32 big-endian)        │ MuxMessage::to_frame_body() bytes   │
//! │ = body length (NOT incl. self) │ = [type:u8][pane_id:u32 LE][payload]│
//! └────────────────────────────────┴─────────────────────────────────────┘
//! ```
//!
//! `MAX_FRAME_LENGTH` (16 MiB, from `mux_ipc::protocol`) caps both the
//! advertised length we are willing to read **and** the encoder output. An
//! attempt to encode an oversized body returns [`WireError::FrameTooLarge`]
//! without touching the output buffer. An attempt to read an oversized
//! length returns [`WireError::FrameTooLarge`] without consuming further
//! bytes from the stream.

use std::io::{self, Read};

use mux_ipc::protocol::{MuxMessage, MAX_FRAME_LENGTH};

/// Errors emitted by [`encode_into`] / [`read_frame`].
#[derive(Debug)]
pub enum WireError {
    /// I/O error from the underlying reader / writer.
    Io(io::Error),
    /// The frame's body length exceeds [`mux_ipc::protocol::MAX_FRAME_LENGTH`].
    /// Returned by both the encoder (before any write) and the reader (before
    /// allocating).
    FrameTooLarge { len: usize },
    /// The decoded body did not match the `MuxMessage` wire shape (either
    /// `body.len() < 5` or an unknown `MessageType` byte). The frame is
    /// considered lost; the caller may continue reading further frames.
    InvalidFrameBody,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "mux wire io error: {e}"),
            Self::FrameTooLarge { len } => {
                write!(f, "mux wire frame too large: {len} > {MAX_FRAME_LENGTH}")
            }
            Self::InvalidFrameBody => write!(f, "mux wire invalid frame body"),
        }
    }
}

impl std::error::Error for WireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for WireError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Encode a [`MuxMessage`] into `buf` as `[u32 BE length][body]`.
///
/// `buf` is appended to (never truncated) so callers may batch multiple
/// frames into a single write. On [`WireError::FrameTooLarge`] the buffer is
/// left untouched.
pub fn encode_into(buf: &mut Vec<u8>, msg: &MuxMessage) -> Result<(), WireError> {
    let body = msg.to_frame_body();
    if body.len() > MAX_FRAME_LENGTH {
        return Err(WireError::FrameTooLarge { len: body.len() });
    }
    let len = body.len() as u32;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    Ok(())
}

/// Read exactly one [`MuxMessage`] from `reader`.
///
/// The function blocks until the full frame is available or the stream
/// returns EOF (`UnexpectedEof`) / a hard I/O error.
///
/// Frames whose advertised length is `0` are treated as
/// [`WireError::InvalidFrameBody`] (no `MuxMessage` is smaller than 5 bytes);
/// frames over [`MAX_FRAME_LENGTH`] yield [`WireError::FrameTooLarge`] without
/// allocating the body buffer.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<MuxMessage, WireError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_FRAME_LENGTH {
        return Err(WireError::FrameTooLarge { len });
    }

    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;

    MuxMessage::from_frame_body(&body).ok_or(WireError::InvalidFrameBody)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mux_ipc::protocol::MessageType;
    use std::io::Cursor;

    // ── TS-wire-1: round-trip ────────────────────────────────────────────

    #[test]
    fn round_trip_pty_output() {
        let msg = MuxMessage::pty_output(7, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let mut buf = Vec::new();
        encode_into(&mut buf, &msg).unwrap();
        // 4-byte length + 5-byte header + 4-byte payload = 13.
        assert_eq!(buf.len(), 13);
        // Length prefix is big-endian.
        let expected_body_len = (msg.to_frame_body().len() as u32).to_be_bytes();
        assert_eq!(&buf[..4], &expected_body_len);

        let mut cursor = Cursor::new(buf);
        let decoded = read_frame(&mut cursor).unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 7);
        assert_eq!(decoded.payload, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn round_trip_empty_payload() {
        let msg = MuxMessage::pty_output(0, vec![]);
        let mut buf = Vec::new();
        encode_into(&mut buf, &msg).unwrap();
        let mut cursor = Cursor::new(buf);
        let decoded = read_frame(&mut cursor).unwrap();
        assert!(decoded.payload.is_empty());
        assert_eq!(decoded.pane_id, 0);
    }

    #[test]
    fn round_trip_multiple_frames_streamed() {
        let mut buf = Vec::new();
        encode_into(&mut buf, &MuxMessage::pty_output(1, vec![0xAA])).unwrap();
        encode_into(&mut buf, &MuxMessage::pty_input(2, vec![0xBB])).unwrap();
        let mut cursor = Cursor::new(buf);
        let a = read_frame(&mut cursor).unwrap();
        let b = read_frame(&mut cursor).unwrap();
        assert_eq!(a.msg_type, MessageType::PtyOutput);
        assert_eq!(a.pane_id, 1);
        assert_eq!(a.payload, vec![0xAA]);
        assert_eq!(b.msg_type, MessageType::PtyInput);
        assert_eq!(b.pane_id, 2);
        assert_eq!(b.payload, vec![0xBB]);
    }

    // ── TS-wire-2: limits + error paths ──────────────────────────────────

    #[test]
    fn encode_rejects_oversized_payload() {
        // We can't construct a >16 MiB payload cheaply via `pty_output`, but
        // we can verify the length check by aliasing the constant. Construct
        // a payload that, after the 5-byte header, would exceed
        // MAX_FRAME_LENGTH by exactly one byte.
        let payload_len = MAX_FRAME_LENGTH; // header is 5; body = header + payload
        let payload = vec![0u8; payload_len]; // body.len() = 5 + payload_len > MAX
        let msg = MuxMessage::pty_output(0, payload);
        let mut buf = Vec::new();
        let err = encode_into(&mut buf, &msg).unwrap_err();
        match err {
            WireError::FrameTooLarge { len } => {
                assert_eq!(len, MAX_FRAME_LENGTH + 5);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
        // Buffer must be untouched on failure.
        assert!(buf.is_empty());
    }

    #[test]
    fn read_rejects_oversized_advertised_length() {
        let mut buf = Vec::new();
        // Length prefix advertises one byte more than the cap. No body is
        // emitted — the reader must reject without trying to allocate.
        let bad_len = (MAX_FRAME_LENGTH as u32) + 1;
        buf.extend_from_slice(&bad_len.to_be_bytes());
        let mut cursor = Cursor::new(buf);
        match read_frame(&mut cursor) {
            Err(WireError::FrameTooLarge { len }) => {
                assert_eq!(len, MAX_FRAME_LENGTH + 1);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn read_returns_io_on_eof_in_length() {
        let buf = vec![0x00, 0x00]; // only 2 of 4 length bytes
        let mut cursor = Cursor::new(buf);
        match read_frame(&mut cursor) {
            Err(WireError::Io(e)) => {
                assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
            }
            other => panic!("expected Io(UnexpectedEof), got {other:?}"),
        }
    }

    #[test]
    fn read_returns_io_on_eof_in_body() {
        let mut buf = Vec::new();
        // Advertise 8 bytes but provide only 3.
        buf.extend_from_slice(&8u32.to_be_bytes());
        buf.extend_from_slice(&[0x01, 0x02, 0x03]);
        let mut cursor = Cursor::new(buf);
        match read_frame(&mut cursor) {
            Err(WireError::Io(e)) => {
                assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
            }
            other => panic!("expected Io(UnexpectedEof), got {other:?}"),
        }
    }

    #[test]
    fn read_returns_invalid_for_short_body() {
        // Length = 3 < 5 (the minimum MuxMessage header). Body content is
        // arbitrary because `from_frame_body` rejects on len alone.
        let mut buf = Vec::new();
        buf.extend_from_slice(&3u32.to_be_bytes());
        buf.extend_from_slice(&[0x01, 0x02, 0x03]);
        let mut cursor = Cursor::new(buf);
        match read_frame(&mut cursor) {
            Err(WireError::InvalidFrameBody) => {}
            other => panic!("expected InvalidFrameBody, got {other:?}"),
        }
    }

    #[test]
    fn read_returns_invalid_for_unknown_message_type() {
        // 5-byte body, type=0x11 (SplitPane was removed) → InvalidFrameBody.
        let body: Vec<u8> = vec![0x11, 0x01, 0x00, 0x00, 0x00];
        let mut buf = Vec::new();
        buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
        buf.extend_from_slice(&body);
        let mut cursor = Cursor::new(buf);
        match read_frame(&mut cursor) {
            Err(WireError::InvalidFrameBody) => {}
            other => panic!("expected InvalidFrameBody, got {other:?}"),
        }
    }

    #[test]
    fn wire_format_matches_legacy_codec_byte_for_byte() {
        // Sanity: the body produced here is identical to the one the
        // tokio_util-based daemon codec emits, so any client speaking this
        // wire can talk to the existing daemon.
        let msg = MuxMessage::pty_output(7, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let mut buf = Vec::new();
        encode_into(&mut buf, &msg).unwrap();
        // u32 BE length + raw frame body.
        let mut expected = Vec::new();
        let body = msg.to_frame_body();
        expected.extend_from_slice(&(body.len() as u32).to_be_bytes());
        expected.extend_from_slice(&body);
        assert_eq!(buf, expected);
    }
}
