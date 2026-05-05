//! Pane state: owns a PTY, reader thread, and bounded channel.

use std::io::Write;
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::ring_buffer::DetachRingBuffer;
use crate::pty::passthrough_scanner::PassthroughScanner;
use crate::pty::visibility::{HIDDEN_PASSTHROUGH_CAPACITY_MUX, RawPassthroughBuffer};

/// Pane identifier.
pub type PaneId = u32;

/// Channel for pane title change notifications (pane_id, new_title).
pub type TitleChangeSender = mpsc::Sender<(PaneId, String)>;

/// Swappable title sender shared between the reader thread and the connection handler.
/// Set to Some(tx) when a GUI client is connected; None when detached.
pub type SharedTitleSender = Arc<StdMutex<Option<TitleChangeSender>>>;

/// Thread-safe shared reference to a shadow VT100 parser.
pub type SharedShadowParser = Arc<StdMutex<vt100::Parser>>;

/// PTY output chunk sent from the reader thread to the mux writer.
pub struct PtyOutputChunk {
    pub pane_id: PaneId,
    pub data: Vec<u8>,
}

/// Bounded channel capacity for PTY output per pane.
pub const PTY_CHANNEL_CAPACITY: usize = 256;

/// Where the PTY reader thread sends output data.
///
/// When a GUI client is connected, output goes directly to the channel.
/// When disconnected, output accumulates in a ring buffer for later replay.
pub enum PaneOutputTarget {
    /// Connected: send output to the GUI via channel.
    Connected(mpsc::Sender<PtyOutputChunk>),
    /// Detached: buffer output in a ring buffer for replay on reattach.
    Detached(DetachRingBuffer),
}

/// Thread-safe shared reference to a pane's output target.
pub type SharedOutputTarget = Arc<StdMutex<PaneOutputTarget>>;

/// Result of `evaluate_output_target`. Carries the resume snapshot bytes
/// when the pane transitions Detached -> Connected so the handler can
/// enqueue them on the same channel before the next reader chunk lands.
pub enum EvalResult {
    /// No state change required; output_target is still correct.
    Unchanged,
    /// Pane was switched into Detached buffering mode.
    SwitchedToDetached,
    /// Pane was switched (back) into Connected mode. The handler must send
    /// `snapshot` on the channel before any subsequent reader chunk.
    ResumeWithSnapshot { snapshot: Vec<u8> },
}

/// Decide the correct `output_target` for `pane` given the current network
/// detach flag and the connection-scoped visible flag, and apply the
/// transition in place.
///
/// - `network_detach == true` OR `visible == false`: Detached.
/// - Both clear: Connected(`owned_tx`). Existing Detached state contributes
///   `shadow_snapshot + ring + raw_passthrough` as a one-shot resume snapshot.
///
/// Identity-scoped: when switching Connected -> Detached we only flip panes
/// whose current `Connected(tx)` is `same_channel(owned_tx)`. Panes already
/// owned by another connection are left alone.
pub fn evaluate_output_target(
    pane: &MuxPane,
    network_detach: bool,
    visible: bool,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
) -> EvalResult {
    let want_connected = !network_detach && visible;
    let mut target = pane.output_target.lock().unwrap();
    match &mut *target {
        PaneOutputTarget::Connected(current_tx) => {
            if want_connected {
                if current_tx.same_channel(owned_tx) {
                    EvalResult::Unchanged
                } else {
                    // Different connection owns this pane. Do not touch.
                    EvalResult::Unchanged
                }
            } else if current_tx.same_channel(owned_tx) {
                *target = PaneOutputTarget::Detached(DetachRingBuffer::new(
                    crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                ));
                EvalResult::SwitchedToDetached
            } else {
                // Owned by another connection — don't clobber.
                EvalResult::Unchanged
            }
        }
        PaneOutputTarget::Detached(ring) => {
            if want_connected {
                let mut snapshot = Vec::new();
                snapshot.extend_from_slice(b"\x1b[H\x1b[2J");
                let screen = pane
                    .shadow_parser
                    .lock()
                    .unwrap()
                    .screen()
                    .contents_formatted();
                snapshot.extend_from_slice(&screen);
                let buffered = ring.read_all();
                snapshot.extend_from_slice(&buffered);
                let passthrough = pane.raw_passthrough.lock().unwrap().read_all();
                snapshot.extend_from_slice(&passthrough);
                ring.clear();
                pane.raw_passthrough.lock().unwrap().clear();
                *target = PaneOutputTarget::Connected(owned_tx.clone());
                EvalResult::ResumeWithSnapshot { snapshot }
            } else {
                EvalResult::Unchanged
            }
        }
    }
}

/// A single terminal pane with its PTY and communication channels.
pub struct MuxPane {
    pub id: PaneId,
    pub cols: u16,
    pub rows: u16,
    /// Shared output target (reader thread writes here, connection handler swaps on detach/reattach).
    pub output_target: SharedOutputTarget,
    /// Writer handle for sending input to the PTY.
    writer: Option<Arc<StdMutex<Box<dyn Write + Send>>>>,
    /// Master PTY handle, retained after reader/writer extraction for resize support.
    master: Option<Box<dyn MasterPty + Send>>,
    /// Whether this pane's PTY has exited.
    pub exited: bool,
    /// Shadow VT100 parser for screen state tracking (used for reattach restoration).
    pub shadow_parser: SharedShadowParser,
    /// Cached working directory from OSC 7 detection.
    pub cwd: Arc<StdMutex<Option<String>>>,
    /// Cached title from OSC 0/2 detection (set by pty_reader_loop).
    pub title: Arc<StdMutex<Option<String>>>,
    /// Swappable title change sender (reader thread reads from this on each title change).
    pub title_sender: SharedTitleSender,
    /// Per-pane raw passthrough buffer for image / Markdown OSC sequences
    /// captured while the pane is detached (network detach OR client hidden).
    /// Drained into the reattach / resume snapshot.
    pub raw_passthrough: Arc<StdMutex<RawPassthroughBuffer>>,
    /// Stateful scanner that pulls passthrough sequences out of the raw PTY
    /// byte stream. Lives alongside `raw_passthrough` because partial
    /// sequences span multiple reader chunks.
    pub passthrough_scanner: Arc<StdMutex<PassthroughScanner>>,
}

impl MuxPane {
    /// Create a new pane (PTY spawn handled by caller).
    pub fn new(
        id: PaneId,
        cols: u16,
        rows: u16,
        output_target: SharedOutputTarget,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
    ) -> Self {
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: Some(master),
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(vt100::Parser::new(rows, cols, 0))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
        }
    }

    /// Write input data to the PTY.
    pub fn write_input(&self, data: &[u8]) -> std::io::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "writer closed"))?;
        let mut w = writer
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        w.write_all(data)?;
        w.flush()
    }

    /// Resize the PTY to the given dimensions.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        let master = self
            .master
            .as_ref()
            .ok_or_else(|| "PTY master closed".to_string())?;
        master
            .resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("PTY resize failed: {}", e))?;
        self.cols = cols;
        self.rows = rows;
        self.shadow_parser.lock().unwrap().set_size(rows, cols);
        Ok(())
    }

    /// Mark PTY as exited.
    pub fn mark_exited(&mut self) {
        self.exited = true;
        self.writer = None;
        self.master = None;
    }

    /// Create a pane without a PTY master, for testing only.
    #[cfg(test)]
    pub fn new_test(id: PaneId, cols: u16, rows: u16, output_target: SharedOutputTarget) -> Self {
        let writer: Box<dyn Write + Send> = Box::new(std::io::sink());
        Self {
            id,
            cols,
            rows,
            output_target,
            writer: Some(Arc::new(StdMutex::new(writer))),
            master: None,
            exited: false,
            shadow_parser: Arc::new(StdMutex::new(vt100::Parser::new(rows, cols, 0))),
            cwd: Arc::new(StdMutex::new(None)),
            title: Arc::new(StdMutex::new(None)),
            title_sender: Arc::new(StdMutex::new(None)),
            raw_passthrough: Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            passthrough_scanner: Arc::new(StdMutex::new(PassthroughScanner::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output_target() -> SharedOutputTarget {
        let (tx, _rx) = mpsc::channel(1);
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
    }

    #[test]
    fn test_resize_fails_without_master() {
        let target = make_output_target();
        let mut pane = MuxPane::new_test(1, 80, 24, target);
        let result = pane.resize(120, 40);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("PTY master closed"));
        // Dimensions should not change on error
        assert_eq!(pane.cols, 80);
        assert_eq!(pane.rows, 24);
    }

    #[test]
    fn test_mark_exited_clears_writer_and_master() {
        let target = make_output_target();
        let mut pane = MuxPane::new_test(1, 80, 24, target);
        assert!(!pane.exited);

        pane.mark_exited();
        assert!(pane.exited);

        // Writing should fail after exit
        let result = pane.write_input(b"hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_input_to_sink() {
        let target = make_output_target();
        let pane = MuxPane::new_test(1, 80, 24, target);
        // sink() writer always succeeds
        assert!(pane.write_input(b"hello world").is_ok());
    }

    #[test]
    fn test_channel_backpressure_full() {
        // Channel capacity 1: second send should fail with Full
        let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
        // First send succeeds
        assert!(
            tx.try_send(PtyOutputChunk {
                pane_id: 1,
                data: vec![1]
            })
            .is_ok()
        );
        // Second send hits backpressure (channel full)
        let result = tx.try_send(PtyOutputChunk {
            pane_id: 1,
            data: vec![2],
        });
        assert!(result.is_err());
        match result {
            Err(mpsc::error::TrySendError::Full(_)) => {} // expected
            _ => panic!("Expected Full error"),
        }
    }

    #[test]
    fn test_channel_closed_detection() {
        let (tx, rx) = mpsc::channel::<PtyOutputChunk>(PTY_CHANNEL_CAPACITY);
        drop(rx); // Close receiver
        let result = tx.try_send(PtyOutputChunk {
            pane_id: 1,
            data: vec![1],
        });
        assert!(result.is_err());
        match result {
            Err(mpsc::error::TrySendError::Closed(_)) => {} // expected
            _ => panic!("Expected Closed error"),
        }
    }

    #[test]
    fn test_bounded_channel_capacity_constant() {
        // Verify the constant is reasonable (not too small, not too large)
        assert!(PTY_CHANNEL_CAPACITY >= 64);
        assert!(PTY_CHANNEL_CAPACITY <= 4096);
    }

    #[cfg(unix)]
    #[test]
    fn test_resize_with_real_pty() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();

        let target = make_output_target();
        let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master);

        let result = pane.resize(120, 40);
        assert!(result.is_ok());
        assert_eq!(pane.cols, 120);
        assert_eq!(pane.rows, 40);
    }

    /// TS-12: detached + visible -> stays Detached.
    #[test]
    fn test_evaluate_output_target_network_detached_visible_stays_detached() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached(
            DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY),
        )));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        // network_detach=true, visible=true -> still hidden by network detach.
        let result = evaluate_output_target(&pane, true, true, &owned_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Detached(_)
        ));
    }

    /// TS-13: identity-scoped Connected -> Detached.
    #[test]
    fn test_evaluate_output_target_identity_scoped_connected_to_detached() {
        let (owner_tx, _rx) = mpsc::channel(16);
        let (other_tx, _other_rx) = mpsc::channel(16);
        // Pane is connected to OTHER client.
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(other_tx)));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        // Caller (owner_tx) tries to detach with hidden visible=false.
        let result = evaluate_output_target(&pane, false, false, &owner_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        // Pane remains Connected (to other client) — identity-scoped guard held.
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
    }

    #[test]
    fn test_evaluate_output_target_owner_can_detach() {
        let (owner_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owner_tx.clone())));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, false, false, &owner_tx);
        assert!(matches!(result, EvalResult::SwitchedToDetached));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Detached(_)
        ));
    }

    /// TS-14: Detached -> Connected returns snapshot bytes including
    /// shadow contents and raw_passthrough.
    #[test]
    fn test_evaluate_output_target_detached_to_connected_returns_snapshot() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let mut ring = DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY);
        ring.write(b"buffered-from-ring");
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached(ring)));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        // Seed shadow with some text.
        pane.shadow_parser.lock().unwrap().process(b"hello-shadow");
        // Seed raw_passthrough.
        pane.raw_passthrough
            .lock()
            .unwrap()
            .append(b"\x1b_Gi=1;ZZ\x1b\\");
        let result = evaluate_output_target(&pane, false, true, &owned_tx);
        match result {
            EvalResult::ResumeWithSnapshot { snapshot } => {
                assert!(snapshot.starts_with(b"\x1b[H\x1b[2J"));
                let s = String::from_utf8_lossy(&snapshot);
                assert!(s.contains("hello-shadow"), "snapshot must include shadow");
                assert!(
                    snapshot
                        .windows(b"buffered-from-ring".len())
                        .any(|w| w == b"buffered-from-ring"),
                    "snapshot must include ring data"
                );
                assert!(
                    s.contains("\u{1b}_Gi=1"),
                    "snapshot must include passthrough"
                );
            }
            _ => panic!("expected ResumeWithSnapshot"),
        }
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
        // raw_passthrough must be cleared after consumption.
        assert_eq!(pane.raw_passthrough.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_evaluate_output_target_already_connected_visible_no_op() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, false, true, &owned_tx);
        assert!(matches!(result, EvalResult::Unchanged));
    }
}
