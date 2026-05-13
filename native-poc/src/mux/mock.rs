//! `#[cfg(test)]` in-memory mock for the mux daemon.
//!
//! Backs the [`super::client::Transport`] trait with a pair of
//! `crossbeam_channel` queues (one per direction) and a buffer that the
//! [`Client::Reader`] reads from. Drives `TS-mux-int-*` integration tests
//! without touching a real Unix socket, so they stay deterministic and
//! Docker-friendly (no `/tmp` race, no leftover sockets across runs).
//!
//! Wire shape:
//!
//! - `Client → Server` writes are decoded by the test helper as full frames
//!   via [`super::wire::read_frame`] and stored on `ScriptedServer::recorded`.
//! - `Server → Client` pushes are encoded via [`super::wire::encode_into`]
//!   into a byte queue the [`ChannelReader`] drains on `Read::read`.

#![cfg(test)]

use std::collections::VecDeque;
use std::io::{self, Cursor};
use std::sync::{Arc, Condvar, Mutex};

use mux_ipc::protocol::MuxMessage;

use super::client::Transport;
use super::wire;

/// Server-side observer paired with a [`ChannelTransport`]. Test code drives
/// it directly (record frames sent by the client, push frames into the
/// client's RX path).
pub struct ScriptedServer {
    /// Frames the client has sent us. Tests assert on this in order.
    recorded: Arc<Mutex<Vec<MuxMessage>>>,
    /// Bytes the client has not yet read. `ChannelReader::read` drains from
    /// the head; `push` appends an encoded frame at the tail.
    inbound: Arc<(Mutex<InboundState>, Condvar)>,
}

/// Inbound state shared between the test thread (writer) and the RX thread
/// (reader). `closed` set to true triggers EOF on the next `read`.
#[derive(Debug, Default)]
struct InboundState {
    buf: VecDeque<u8>,
    closed: bool,
}

impl ScriptedServer {
    /// Returns a snapshot of frames the client has sent so far. The lock is
    /// released before returning, so tests can call this in tight loops
    /// without deadlocking the transport.
    pub fn recorded_frames(&self) -> Vec<MuxMessage> {
        // `MuxMessage` is `Clone`.
        self.recorded.lock().unwrap().clone()
    }

    /// Push `msg` into the client's RX queue. Encoded with the same
    /// length-prefix wire format as the real daemon.
    pub fn push(&self, msg: MuxMessage) {
        let mut buf = Vec::new();
        wire::encode_into(&mut buf, &msg).expect("mock encode");
        let (lock, cvar) = &*self.inbound;
        let mut state = lock.lock().unwrap();
        state.buf.extend(buf);
        cvar.notify_all();
    }

    /// Close the inbound stream so the client's RX thread observes EOF.
    pub fn close(&self) {
        let (lock, cvar) = &*self.inbound;
        let mut state = lock.lock().unwrap();
        state.closed = true;
        cvar.notify_all();
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        // Closing on drop guarantees `TS-mux-int-3` sees an EOF even if a
        // test forgets to call `close` explicitly.
        self.close();
    }
}

/// Channel-backed transport for the unit tests. Implements
/// [`super::client::Transport`] by storing writes for inspection and
/// returning a [`ChannelReader`] whose `Read::read` blocks until the test
/// pushes data via [`ScriptedServer::push`].
#[derive(Clone)]
pub struct ChannelTransport {
    recorded: Arc<Mutex<Vec<MuxMessage>>>,
    inbound: Arc<(Mutex<InboundState>, Condvar)>,
    /// Reader half — taken once by `split_reader`. We wrap in a Mutex so the
    /// transport stays `Send + Sync` and the test thread can clone it
    /// trivially.
    reader_slot: Arc<Mutex<Option<ChannelReader>>>,
    /// Pending bytes from the client side that we have not yet decoded into
    /// `recorded`. Accumulates across `write_all` calls because a single
    /// write may contain a partial frame.
    write_buf: Arc<Mutex<Vec<u8>>>,
}

impl Transport for ChannelTransport {
    type Reader = ChannelReader;

    fn split_reader(&self) -> io::Result<Self::Reader> {
        self.reader_slot
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "reader already taken"))
    }

    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        // Append and try to decode any complete frames.
        let mut write_buf = self.write_buf.lock().unwrap();
        write_buf.extend_from_slice(buf);
        // Decode greedily.
        loop {
            if write_buf.len() < 4 {
                break;
            }
            let len = u32::from_be_bytes([write_buf[0], write_buf[1], write_buf[2], write_buf[3]])
                as usize;
            if write_buf.len() < 4 + len {
                break;
            }
            // We have a complete frame.
            let frame_end = 4 + len;
            let frame_bytes: Vec<u8> = write_buf.drain(..frame_end).collect();
            let mut cursor = Cursor::new(frame_bytes);
            match wire::read_frame(&mut cursor) {
                Ok(msg) => {
                    self.recorded.lock().unwrap().push(msg);
                }
                Err(_e) => {
                    // Treat decode failure as test setup bug.
                    return Err(io::Error::other("mock frame decode error"));
                }
            }
        }
        Ok(())
    }

    fn shutdown(&self) {
        // Force the reader to wake up with EOF.
        let (lock, cvar) = &*self.inbound;
        let mut state = lock.lock().unwrap();
        state.closed = true;
        cvar.notify_all();
    }
}

/// Reader half returned by [`ChannelTransport::split_reader`]. Blocks on
/// `read` until data is available or the server has closed.
pub struct ChannelReader {
    inbound: Arc<(Mutex<InboundState>, Condvar)>,
}

impl io::Read for ChannelReader {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        let (lock, cvar) = &*self.inbound;
        let mut state = lock.lock().unwrap();
        loop {
            if !state.buf.is_empty() {
                let n = dst.len().min(state.buf.len());
                for slot in dst.iter_mut().take(n) {
                    *slot = state.buf.pop_front().unwrap();
                }
                return Ok(n);
            }
            if state.closed {
                return Ok(0); // EOF — `read_exact` translates to UnexpectedEof.
            }
            state = cvar.wait(state).unwrap();
        }
    }
}

/// Construct a fresh `(ChannelTransport, ScriptedServer)` pair.
pub fn pair() -> (ChannelTransport, ScriptedServer) {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let inbound = Arc::new((Mutex::new(InboundState::default()), Condvar::new()));
    let reader = ChannelReader {
        inbound: inbound.clone(),
    };
    let transport = ChannelTransport {
        recorded: recorded.clone(),
        inbound: inbound.clone(),
        reader_slot: Arc::new(Mutex::new(Some(reader))),
        write_buf: Arc::new(Mutex::new(Vec::new())),
    };
    let server = ScriptedServer { recorded, inbound };
    (transport, server)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mux_ipc::protocol::MessageType;

    #[test]
    fn pair_round_trips_one_frame() {
        let (transport, server) = pair();
        // Server pushes a status update.
        let msg = MuxMessage::pty_output(7, vec![1, 2, 3]);
        server.push(msg.clone());

        // Reader half drains it.
        let mut reader = transport.split_reader().unwrap();
        let got = wire::read_frame(&mut reader).unwrap();
        assert_eq!(got.msg_type, MessageType::PtyOutput);
        assert_eq!(got.pane_id, 7);
        assert_eq!(got.payload, vec![1, 2, 3]);
    }

    #[test]
    fn client_writes_are_recorded() {
        let (transport, server) = pair();
        let msg = MuxMessage::pty_input(2, vec![0xAB]);
        let mut buf = Vec::new();
        wire::encode_into(&mut buf, &msg).unwrap();
        transport.write_all(&buf).unwrap();
        let frames = server.recorded_frames();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].msg_type, MessageType::PtyInput);
    }

    #[test]
    fn close_makes_reader_return_eof() {
        let (transport, server) = pair();
        let mut reader = transport.split_reader().unwrap();
        server.close();
        let mut buf = [0u8; 16];
        // After close + no pending data, read returns 0 (EOF).
        let n = std::io::Read::read(&mut reader, &mut buf).unwrap();
        assert_eq!(n, 0);
    }
}
