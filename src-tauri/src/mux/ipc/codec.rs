//! Length-delimited codec for MuxMessage framing over IPC sockets.
//!
//! Wraps tokio_util's LengthDelimitedCodec to encode/decode MuxMessage
//! frames with a u32 length prefix and MAX_FRAME_LENGTH limit.

use bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use super::protocol::{MAX_FRAME_LENGTH, MuxMessage};

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
        match self.inner.decode(src)? {
            Some(frame) => MuxMessage::from_frame_body(&frame)
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid mux frame")
                })
                .map(Some),
            None => Ok(None),
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
    use super::super::protocol::MessageType;
    use super::*;

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
    fn test_codec_empty_payload() {
        let mut codec = MuxCodec::new();
        let msg = MuxMessage::pty_output(0, vec![]);

        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert!(decoded.payload.is_empty());
    }
}
