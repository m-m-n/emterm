//! Blocking `UnixStream` mux client.
//!
//! Owns one `UnixStream` plus a single RX thread. The RX thread reads frames
//! via [`super::wire::read_frame`] in a loop and forwards typed
//! `mux_ipc::protocol::MuxMessage` values over an `mpsc::Sender` to the main
//! thread. The send side is mutex-guarded so the main thread can issue
//! requests from anywhere without ordering hazards.
//!
//! Connection abort (daemon dies mid-session) is observable via
//! [`Client::try_recv`] returning [`ChannelEvent::Closed`] so the app layer
//! can fall back to native PTY mode.

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use mux_ipc::protocol::{
    AttachMsg, ClientType, HelloMsg, MessageType, MuxMessage, PROTOCOL_VERSION,
};

use super::wire::{self, WireError};

/// What the RX thread surfaces on each `try_recv`. The `Closed` variant lets
/// the app layer recover (drop the client, resume native PTY mode) without
/// having to inspect a thread join handle.
#[derive(Debug)]
pub enum ChannelEvent {
    /// A frame arrived from the daemon.
    Message(MuxMessage),
    /// The RX thread exited. Either the daemon closed the connection or an
    /// unrecoverable framing error occurred. `reason` carries a short
    /// human-readable description for the log line.
    Closed { reason: String },
}

/// Connection-time errors.
#[derive(Debug)]
pub enum ConnectError {
    /// `UnixStream::connect` failed.
    Connect(std::io::Error),
    /// The handshake `Hello` could not be sent.
    Handshake(WireError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "mux client connect failed: {e}"),
            Self::Handshake(e) => write!(f, "mux client handshake failed: {e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// Send-side errors.
#[derive(Debug)]
pub enum SendError {
    /// The encoder rejected the message (oversized payload).
    Encode(WireError),
    /// `write_all` on the underlying socket failed.
    Io(std::io::Error),
    /// The client has already shut down.
    Closed,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "mux client encode failed: {e}"),
            Self::Io(e) => write!(f, "mux client write failed: {e}"),
            Self::Closed => write!(f, "mux client already shut down"),
        }
    }
}

impl std::error::Error for SendError {}

/// The transport an RX/TX pair operates on. Production uses `UnixStream`;
/// tests use [`super::mock::ChannelTransport`] so they can simulate the
/// daemon end-to-end without a real socket.
pub trait Transport: Send + 'static {
    /// Type returned by [`Transport::split_reader`].
    type Reader: std::io::Read + Send + 'static;

    /// Take ownership of a reader half. Called once at construction so the
    /// RX thread can `read_exact` without contending with the TX side.
    fn split_reader(&self) -> std::io::Result<Self::Reader>;

    /// Write `buf` to the transport. Called from the main thread under a
    /// mutex; implementations should be cheap to clone or share via `Arc`.
    fn write_all(&self, buf: &[u8]) -> std::io::Result<()>;

    /// Shut the transport down so the RX thread observes EOF and exits.
    fn shutdown(&self);
}

/// Newtype around `Arc<Mutex<UnixStream>>` so the `Transport` impl is
/// orphan-free.
#[derive(Clone)]
pub struct UnixTransport {
    write_half: Arc<Mutex<UnixStream>>,
    read_clone: Arc<Mutex<Option<UnixStream>>>,
}

impl UnixTransport {
    fn new(stream: UnixStream) -> std::io::Result<Self> {
        // `UnixStream` is bidirectional; we need a separate clone for the RX
        // thread because `read_exact` and `write_all` would otherwise block
        // each other behind the same `&mut`.
        let read_clone = stream.try_clone()?;
        Ok(Self {
            write_half: Arc::new(Mutex::new(stream)),
            read_clone: Arc::new(Mutex::new(Some(read_clone))),
        })
    }
}

impl Transport for UnixTransport {
    type Reader = UnixStream;

    fn split_reader(&self) -> std::io::Result<UnixStream> {
        let mut slot = self.read_clone.lock().unwrap();
        slot.take().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "reader half already taken",
            )
        })
    }

    fn write_all(&self, buf: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let mut guard = self.write_half.lock().unwrap();
        guard.write_all(buf)
    }

    fn shutdown(&self) {
        let guard = self.write_half.lock().unwrap();
        // Best-effort: tell the kernel to refuse further reads + writes so
        // the RX thread wakes up out of `read_exact` with EOF.
        let _ = guard.shutdown(std::net::Shutdown::Both);
    }
}

/// Mux client. Generic over the [`Transport`] so the integration tests can
/// swap the real `UnixStream` for a channel-backed mock.
pub struct Client<T: Transport = UnixTransport> {
    transport: T,
    rx: Receiver<ChannelEvent>,
    rx_join: Option<JoinHandle<()>>,
    /// Cached session ID so callers can re-query it for the tab title without
    /// snooping inside the connect path.
    session_id: String,
    /// True once `shutdown` has been called; further `send` calls fail with
    /// [`SendError::Closed`].
    closed: bool,
}

impl Client<UnixTransport> {
    /// Open a blocking `UnixStream` to `socket`, send the handshake `Hello`
    /// + the per-session `Attach` payload, and spawn the RX thread.
    ///
    /// The caller is expected to have already validated `socket` and
    /// `session_id` via [`super::osc777::parse`].
    pub fn connect<P: AsRef<Path>>(socket: P, session_id: String) -> Result<Self, ConnectError> {
        let stream = UnixStream::connect(socket.as_ref()).map_err(ConnectError::Connect)?;
        let transport = UnixTransport::new(stream).map_err(ConnectError::Connect)?;
        Self::connect_with_transport(transport, session_id)
    }
}

impl<T: Transport> Client<T> {
    /// Construct a client over an arbitrary transport, performing the same
    /// handshake the real client does. Used by tests via the mock transport.
    pub fn connect_with_transport(transport: T, session_id: String) -> Result<Self, ConnectError> {
        // 1. Send the Hello handshake. The protocol expects bincode-encoded
        //    `HelloMsg` wrapped in `MuxMessage::control(MessageType::Hello, …)`.
        let hello = HelloMsg {
            client_type: ClientType::Gui,
            protocol_version: PROTOCOL_VERSION,
        };
        let hello_msg = MuxMessage::control(MessageType::Hello, 0, &hello);
        let mut buf = Vec::new();
        wire::encode_into(&mut buf, &hello_msg).map_err(ConnectError::Handshake)?;

        // 2. Send the Attach with a placeholder session_id u32. The daemon's
        //    OSC 777 flow keys the per-session attach on the session name
        //    (carried by the AttachMsg payload's serde encoding); the legacy
        //    `AttachMsg` only has `session_id: u32` so we currently encode 0
        //    here and rely on the daemon to read the session by name from a
        //    side-channel header. This matches how the legacy WebView build
        //    constructs the message — see `src-tauri/src/mux/bridge.rs`.
        //
        //    NOTE: A future protocol bump may replace u32 with a name-based
        //    `AttachByName { name: String }`. For now we keep the legacy
        //    shape so Phase 4-C can talk to an unmodified daemon.
        let attach = AttachMsg { session_id: 0 };
        let attach_msg = MuxMessage::control(MessageType::Attach, 0, &attach);
        wire::encode_into(&mut buf, &attach_msg).map_err(ConnectError::Handshake)?;

        transport
            .write_all(&buf)
            .map_err(|e| ConnectError::Handshake(WireError::Io(e)))?;

        // 3. Spawn the RX thread.
        let reader = transport.split_reader().map_err(ConnectError::Connect)?;
        let (tx, rx) = mpsc::channel();
        let rx_join = std::thread::Builder::new()
            .name("native-poc-mux-rx".into())
            .spawn(move || rx_loop(reader, tx))
            .map_err(ConnectError::Connect)?;

        Ok(Self {
            transport,
            rx,
            rx_join: Some(rx_join),
            session_id,
            closed: false,
        })
    }

    /// Session ID this client was constructed with. Stored for tab-title
    /// rendering.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Send one control / data message to the daemon.
    pub fn send(&self, msg: &MuxMessage) -> Result<(), SendError> {
        if self.closed {
            return Err(SendError::Closed);
        }
        let mut buf = Vec::new();
        wire::encode_into(&mut buf, msg).map_err(SendError::Encode)?;
        self.transport.write_all(&buf).map_err(SendError::Io)
    }

    /// Non-blocking poll of the RX channel. Returns `None` when no frame is
    /// available, and `Some(ChannelEvent::Closed)` exactly once when the RX
    /// thread exits.
    pub fn try_recv(&self) -> Option<ChannelEvent> {
        match self.rx.try_recv() {
            Ok(evt) => Some(evt),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(ChannelEvent::Closed {
                reason: "RX channel disconnected".into(),
            }),
        }
    }

    /// Tear down the client. Closes the transport and joins the RX thread.
    /// Idempotent — calling again after shutdown is a no-op.
    pub fn shutdown(mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.transport.shutdown();
        if let Some(h) = self.rx_join.take() {
            let _ = h.join();
        }
    }
}

impl<T: Transport> Drop for Client<T> {
    fn drop(&mut self) {
        // Ensure the RX thread terminates even on an unexpected drop.
        if !self.closed {
            self.closed = true;
            self.transport.shutdown();
            if let Some(h) = self.rx_join.take() {
                let _ = h.join();
            }
        }
    }
}

/// RX thread body. Reads frames in a loop and forwards them; on any error or
/// EOF, emits a single `Closed` event and exits.
fn rx_loop<R: std::io::Read>(mut reader: R, tx: Sender<ChannelEvent>) {
    loop {
        match wire::read_frame(&mut reader) {
            Ok(msg) => {
                if tx.send(ChannelEvent::Message(msg)).is_err() {
                    // Main thread dropped the receiver; nothing to do.
                    return;
                }
            }
            Err(WireError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                let _ = tx.send(ChannelEvent::Closed {
                    reason: "daemon closed connection".into(),
                });
                return;
            }
            Err(e) => {
                let _ = tx.send(ChannelEvent::Closed {
                    reason: format!("{e}"),
                });
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::mock::{self, ScriptedServer};

    fn drain(client: &Client<mock::ChannelTransport>, mut max: usize) -> Vec<ChannelEvent> {
        let mut out = Vec::new();
        while max > 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            if let Some(ev) = client.try_recv() {
                out.push(ev);
            }
            max -= 1;
        }
        out
    }

    // ── TS-mux-int-1: handshake round-trip ───────────────────────────────

    #[test]
    fn handshake_sends_hello_and_attach() {
        let (transport, server) = mock::pair();
        let client = Client::connect_with_transport(transport, "session-a".into()).unwrap();
        assert_eq!(client.session_id(), "session-a");

        // The server should have observed exactly two frames: Hello, Attach.
        // The mock collects writes immediately on `write_all` so we don't
        // need to sleep.
        let frames = server.recorded_frames();
        assert_eq!(frames.len(), 2, "got {} frames", frames.len());
        assert_eq!(frames[0].msg_type, MessageType::Hello);
        assert_eq!(frames[1].msg_type, MessageType::Attach);

        client.shutdown();
    }

    // ── TS-mux-int-2: server-to-client frame round-trip ──────────────────

    #[test]
    fn server_pushed_status_update_arrives_via_try_recv() {
        use mux_ipc::protocol::StatusUpdateMsg;
        let (transport, server) = mock::pair();
        let client = Client::connect_with_transport(transport, "sid".into()).unwrap();

        let status = StatusUpdateMsg {
            left: "[mux] window-0".into(),
            right: "12:00".into(),
        };
        server.push(MuxMessage::control(MessageType::StatusUpdate, 1, &status));

        let mut events = Vec::new();
        for _ in 0..20 {
            if let Some(ev) = client.try_recv() {
                events.push(ev);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(events.len(), 1);
        match &events[0] {
            ChannelEvent::Message(m) => {
                assert_eq!(m.msg_type, MessageType::StatusUpdate);
                let decoded: StatusUpdateMsg = m.decode_payload().unwrap();
                assert_eq!(decoded.left, "[mux] window-0");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        client.shutdown();
    }

    // ── TS-mux-int-3: clean shutdown surfaces Closed ─────────────────────

    #[test]
    fn server_disconnect_emits_closed_event() {
        let (transport, server) = mock::pair();
        let client = Client::connect_with_transport(transport, "sid".into()).unwrap();

        // Drop the server end — RX thread should see EOF and emit `Closed`.
        drop(server);

        let events = drain(&client, 40);
        let closed = events
            .iter()
            .any(|e| matches!(e, ChannelEvent::Closed { .. }));
        assert!(closed, "expected ChannelEvent::Closed, got: {events:?}");

        client.shutdown();
    }

    // ── TS-mux-int-4: client send round-trips through the transport ──────

    #[test]
    fn client_send_is_visible_to_server() {
        let (transport, server) = mock::pair();
        let client = Client::connect_with_transport(transport, "sid".into()).unwrap();
        // After handshake, send a SwitchWindow request.
        let msg = MuxMessage::control(MessageType::SwitchWindow, 0, &"next".to_string());
        client.send(&msg).unwrap();
        let frames = server.recorded_frames();
        // Frames: [Hello, Attach, SwitchWindow]
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[2].msg_type, MessageType::SwitchWindow);

        client.shutdown();
    }
}
