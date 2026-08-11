//! Length-delimited codec for MuxMessage framing over IPC sockets.
//!
//! Wraps tokio_util's LengthDelimitedCodec to encode/decode MuxMessage
//! frames with a u32 length prefix and MAX_FRAME_LENGTH limit.

use bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use mux_ipc::protocol::{MAX_FRAME_LENGTH, MuxMessage};

/// Codec that frames MuxMessages with length-delimited encoding.
pub struct MuxCodec {
    inner: LengthDelimitedCodec,
}

impl MuxCodec {
    pub fn new() -> Self {
        Self {
            inner: LengthDelimitedCodec::builder()
                .length_field_length(4)
                .max_frame_length(MAX_FRAME_LENGTH)
                .new_codec(),
        }
    }
}

impl Default for MuxCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for MuxCodec {
    type Item = MuxMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match self.inner.decode(src)? {
                Some(frame) => match MuxMessage::from_frame_body(&frame) {
                    Some(msg) => return Ok(Some(msg)),
                    None => {
                        let type_byte = frame.first().copied().unwrap_or(0);
                        log::warn!(
                            "Discarding unknown/malformed mux frame: type=0x{:02x} len={}",
                            type_byte,
                            frame.len()
                        );
                    }
                },
                None => return Ok(None),
            }
        }
    }
}

impl Encoder<MuxMessage> for MuxCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: MuxMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let body = item.to_frame_body();
        self.inner.encode(Bytes::from(body), dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mux_ipc::protocol::MessageType;

    #[test]
    fn test_codec_round_trip() {
        let mut codec = MuxCodec::new();
        let msg = MuxMessage::pty_output(7, vec![0xDE, 0xAD, 0xBE, 0xEF]);

        // Encode
        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        // Decode
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 7);
        assert_eq!(decoded.payload, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_codec_partial_frame() {
        let mut codec = MuxCodec::new();
        let msg = MuxMessage::pty_output(1, vec![1, 2, 3]);

        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        // Give only partial data
        let full = buf.split();
        let mut partial = BytesMut::from(&full[..3]);
        assert!(codec.decode(&mut partial).unwrap().is_none());
    }

    #[test]
    fn test_codec_multiple_messages() {
        let mut codec = MuxCodec::new();
        let mut buf = BytesMut::new();

        // Encode two messages
        codec
            .encode(MuxMessage::pty_output(1, vec![0xAA]), &mut buf)
            .unwrap();
        codec
            .encode(MuxMessage::pty_input(2, vec![0xBB]), &mut buf)
            .unwrap();

        // Decode both
        let msg1 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg1.msg_type, MessageType::PtyOutput);
        assert_eq!(msg1.pane_id, 1);

        let msg2 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg2.msg_type, MessageType::PtyInput);
        assert_eq!(msg2.pane_id, 2);

        // No more
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_codec_unknown_frame_is_discarded_not_fatal() {
        let mut codec = MuxCodec::new();
        let mut buf = BytesMut::new();

        // Manually craft a frame with unknown message type 0x11 (removed SplitPane)
        // Format: [length: u32 big-endian][type: u8][pane_id: u32 LE][payload]
        let body: Vec<u8> = vec![0x11, 0x01, 0x00, 0x00, 0x00, 0xDE, 0xAD];
        buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
        buf.extend_from_slice(&body);

        // Then a valid PtyOutput frame immediately after
        codec
            .encode(MuxMessage::pty_output(2, vec![0xBB]), &mut buf)
            .unwrap();

        // The unknown frame should be silently discarded, and decode should
        // return the next valid frame (PtyOutput) instead of erroring out.
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 2);
        assert_eq!(decoded.payload, vec![0xBB]);
    }

    /// AC-3 (mux-status-bar-removal task0001, FR8a): the daemon's receive
    /// path (this codec) tolerates a raw frame carrying either retired
    /// opcode (0x16 / 0x17 -- reserved, never reused) exactly like any
    /// other unrecognized message type: discarded with at most a warn
    /// log, connection stays up, decoding continues with the next valid
    /// frame. Built as raw bytes (not through the typed `MuxMessage` API,
    /// which can no longer name the retired types) so this test stays
    /// valid regardless of whether those types still exist anywhere in
    /// the tree.
    #[test]
    fn test_codec_retired_status_bar_opcodes_are_discarded_not_fatal() {
        let mut codec = MuxCodec::new();
        let mut buf = BytesMut::new();

        for retired_type in [0x16u8, 0x17u8] {
            let body: Vec<u8> = vec![retired_type, 0x00, 0x00, 0x00, 0x00];
            buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
            buf.extend_from_slice(&body);
        }

        // Then a valid PtyOutput frame immediately after both.
        codec
            .encode(MuxMessage::pty_output(9, vec![0xEE]), &mut buf)
            .unwrap();

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 9);
        assert_eq!(decoded.payload, vec![0xEE]);
    }

    #[test]
    fn test_codec_short_frame_is_discarded_not_fatal() {
        let mut codec = MuxCodec::new();
        let mut buf = BytesMut::new();

        // Frame shorter than 5 bytes (type + pane_id minimum) must not kill the stream
        let body: Vec<u8> = vec![0x01, 0x02];
        buf.extend_from_slice(&(body.len() as u32).to_be_bytes());
        buf.extend_from_slice(&body);

        codec
            .encode(MuxMessage::pty_output(3, vec![0xCC]), &mut buf)
            .unwrap();

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 3);
    }

    #[test]
    fn test_codec_empty_payload() {
        let mut codec = MuxCodec::new();
        let msg = MuxMessage::pty_output(0, vec![]);

        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert!(decoded.payload.is_empty());
    }
}
