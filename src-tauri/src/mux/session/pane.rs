//! Pane state: owns a PTY, reader thread, and bounded channel.

use std::io::Write;
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::scrollback_buffer::{DEFAULT_SCROLLBACK_CAPACITY, ScrollbackRingBuffer};
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

/// Why a pane is currently detached. Combines `NetworkDetach`
/// (no client connected / kicked / explicit detach) with
/// `HiddenByVisibility` (client connected but reported hidden).
///
/// A pane stays Detached until **all** active reasons clear:
/// - hidden -> visible resolves the `HiddenByVisibility` bit
/// - reattach resolves the `NetworkDetach` bit
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachReason {
    NetworkDetach,
    HiddenByVisibility,
    Both,
}

impl DetachReason {
    pub fn has_network(self) -> bool {
        matches!(self, DetachReason::NetworkDetach | DetachReason::Both)
    }

    pub fn has_hidden(self) -> bool {
        matches!(self, DetachReason::HiddenByVisibility | DetachReason::Both)
    }

    /// Combine two reasons (set union).
    pub fn combine(a: Self, b: Self) -> Self {
        match (
            a.has_network() || b.has_network(),
            a.has_hidden() || b.has_hidden(),
        ) {
            (true, true) => DetachReason::Both,
            (true, false) => DetachReason::NetworkDetach,
            (false, true) => DetachReason::HiddenByVisibility,
            (false, false) => DetachReason::HiddenByVisibility,
        }
    }

    /// Clear the `NetworkDetach` bit. Returns `None` when the result is empty.
    pub fn clear_network(self) -> Option<Self> {
        match self {
            DetachReason::NetworkDetach => None,
            DetachReason::HiddenByVisibility => Some(DetachReason::HiddenByVisibility),
            DetachReason::Both => Some(DetachReason::HiddenByVisibility),
        }
    }

    /// Clear the `HiddenByVisibility` bit. Returns `None` when the result is empty.
    pub fn clear_hidden(self) -> Option<Self> {
        match self {
            DetachReason::HiddenByVisibility => None,
            DetachReason::NetworkDetach => Some(DetachReason::NetworkDetach),
            DetachReason::Both => Some(DetachReason::NetworkDetach),
        }
    }
}

/// Where the PTY reader thread sends output data.
///
/// When a GUI client is connected, output goes directly to the channel.
/// When disconnected, the reader still drains the PTY but suppresses sends;
/// the scrollback ring (Phase B: `MuxPane::scrollback`) captures recent
/// bytes for replay on reattach.
///
/// `Detached` carries identity (`owner`) and cause (`reason`) so a second
/// connection cannot reclaim a pane that the first connection put into
/// `HiddenByVisibility`. `owner = None` marks system-origin detaches (e.g.
/// pane spawned before any client attached, or PTY EOF fallback) which any
/// connection may reclaim.
pub enum PaneOutputTarget {
    /// Connected: send output to the GUI via channel.
    Connected(mpsc::Sender<PtyOutputChunk>),
    /// Detached: the reader keeps draining the PTY into the per-pane
    /// scrollback (`MuxPane::scrollback`) and `raw_passthrough`; no
    /// channel send happens until the pane returns to `Connected`.
    Detached {
        reason: DetachReason,
        owner: Option<mpsc::Sender<PtyOutputChunk>>,
    },
}

/// Thread-safe shared reference to a pane's output target.
pub type SharedOutputTarget = Arc<StdMutex<PaneOutputTarget>>;

/// Thread-safe shared reference to a pane's scrollback ring buffer.
pub type SharedScrollback = Arc<StdMutex<ScrollbackRingBuffer>>;

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

/// Outcome of `resume_pane_with_permit`. Mirrors the in-lock decision so
/// callers can branch on the side effects (snapshot enqueued + Connected
/// swap, or no transition) without re-locking the pane.
pub enum ResumeOutcome {
    /// Pane was Detached and resolved cleanly. The snapshot was sent via
    /// the supplied permit and the target was swapped to `Connected`.
    Resumed,
    /// No transition: pane is already Connected, owner mismatches, or
    /// `NetworkDetach` is still active (only reattach can clear it).
    NoChange,
}

/// Decide the correct `output_target` for `pane` given the current network
/// detach flag and the connection-scoped visible flag, and apply the
/// transition in place.
///
/// Identity-scoped on both edges:
/// - Connected -> Detached: only flip panes whose current `Connected(tx)`
///   is `same_channel(owned_tx)`. The new `Detached` records `owned_tx` as
///   `owner` so a different connection cannot reclaim it via SetVisibility.
/// - Detached -> Connected: only resume when `owner` matches `owned_tx`
///   (or `owner == None`, system origin), AND the resolved reason becomes
///   empty after the caller's transition (`HiddenByVisibility` clears on
///   `visible=true`; `NetworkDetach` only clears via the reattach path).
pub fn evaluate_output_target(
    pane: &MuxPane,
    network_detach: bool,
    visible: bool,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
) -> EvalResult {
    let mut target = pane.output_target.lock().unwrap();
    let new_reason = match (network_detach, visible) {
        (true, true) => Some(DetachReason::NetworkDetach),
        (false, false) => Some(DetachReason::HiddenByVisibility),
        (true, false) => Some(DetachReason::Both),
        (false, true) => None,
    };
    match &mut *target {
        PaneOutputTarget::Connected(current_tx) => {
            let owned_by_caller = current_tx.same_channel(owned_tx);
            match new_reason {
                None => EvalResult::Unchanged,
                Some(reason) if owned_by_caller => {
                    *target = PaneOutputTarget::Detached {
                        reason,
                        owner: Some(owned_tx.clone()),
                    };
                    EvalResult::SwitchedToDetached
                }
                Some(_) => EvalResult::Unchanged,
            }
        }
        PaneOutputTarget::Detached { reason, owner } => {
            let owner_matches = match owner {
                Some(o) => o.same_channel(owned_tx),
                None => true,
            };
            if !owner_matches {
                return EvalResult::Unchanged;
            }
            // Resolve current reason against the caller's transition.
            let resolved = if visible {
                reason.clear_hidden()
            } else {
                Some(*reason)
            };
            let resolved = match (network_detach, resolved) {
                (true, Some(r)) => Some(DetachReason::combine(r, DetachReason::NetworkDetach)),
                (true, None) => Some(DetachReason::NetworkDetach),
                (false, r) => r,
            };
            match resolved {
                Some(r) => {
                    *reason = r;
                    if owner.is_none() {
                        *owner = Some(owned_tx.clone());
                    }
                    EvalResult::Unchanged
                }
                None => {
                    // Phase C FR5 order: clear → scrollback → shadow →
                    // passthrough. Scrollback is read WITHOUT clearing (FR6:
                    // the buffer lives for the lifetime of the pane).
                    let screen = pane
                        .shadow_parser
                        .lock()
                        .unwrap()
                        .screen()
                        .contents_formatted();
                    let buffered = pane.scrollback.lock().unwrap().read_all();
                    let passthrough = {
                        let mut buf = pane.raw_passthrough.lock().unwrap();
                        let bytes = buf.read_all();
                        buf.clear();
                        bytes
                    };
                    let mut snapshot =
                        Vec::with_capacity(8 + buffered.len() + screen.len() + passthrough.len());
                    snapshot.extend_from_slice(b"\x1b[H\x1b[2J");
                    snapshot.extend_from_slice(&buffered);
                    snapshot.extend_from_slice(&screen);
                    snapshot.extend_from_slice(&passthrough);
                    *target = PaneOutputTarget::Connected(owned_tx.clone());
                    EvalResult::ResumeWithSnapshot { snapshot }
                }
            }
        }
    }
}

/// FR9 race-free Detached -> Connected resume.
///
/// The caller obtains an `mpsc::Permit` for `pane_output_tx` *outside* the
/// pane lock (via `Sender::reserve().await`), then hands it in here. This
/// function holds the pane's `output_target` mutex for the full lifetime of
/// (build snapshot, send via permit, swap to `Connected`). Because the PTY
/// reader thread also takes the same `output_target` mutex before its
/// `try_send` / `blocking_send`, the reader cannot push a live chunk between
/// the snapshot enqueue and the Connected swap — the snapshot is guaranteed
/// to land first in the channel's FIFO.
///
/// `Permit::send` is consumed and infallible (the slot is already reserved),
/// so the entire sequence runs under the std mutex without `await`.
///
/// Returns `ResumeOutcome::NoChange` when the pane is not eligible to
/// resume (already Connected, owner mismatch, or `NetworkDetach` still
/// active). The caller should drop the permit on `NoChange` to release the
/// reserved slot.
pub fn resume_pane_with_permit(
    pane: &MuxPane,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
    permit: mpsc::Permit<'_, PtyOutputChunk>,
) -> ResumeOutcome {
    let mut target = pane.output_target.lock().unwrap();
    match &mut *target {
        PaneOutputTarget::Connected(_) => ResumeOutcome::NoChange,
        PaneOutputTarget::Detached { reason, owner } => {
            let owner_matches = match owner {
                Some(o) => o.same_channel(owned_tx),
                None => true,
            };
            if !owner_matches {
                return ResumeOutcome::NoChange;
            }
            let resolved = reason.clear_hidden();
            if let Some(r) = resolved {
                *reason = r;
                if owner.is_none() {
                    *owner = Some(owned_tx.clone());
                }
                return ResumeOutcome::NoChange;
            }
            // Phase C FR5 order: clear → scrollback → shadow → passthrough.
            // Scrollback is read WITHOUT clearing (FR6).
            let screen = pane
                .shadow_parser
                .lock()
                .unwrap()
                .screen()
                .contents_formatted();
            let buffered = pane.scrollback.lock().unwrap().read_all();
            let passthrough = {
                let mut buf = pane.raw_passthrough.lock().unwrap();
                let bytes = buf.read_all();
                buf.clear();
                bytes
            };
            let mut snapshot =
                Vec::with_capacity(8 + buffered.len() + screen.len() + passthrough.len());
            snapshot.extend_from_slice(b"\x1b[H\x1b[2J");
            snapshot.extend_from_slice(&buffered);
            snapshot.extend_from_slice(&screen);
            snapshot.extend_from_slice(&passthrough);
            permit.send(PtyOutputChunk {
                pane_id: pane.id,
                data: snapshot,
            });
            *target = PaneOutputTarget::Connected(owned_tx.clone());
            ResumeOutcome::Resumed
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
    /// Per-pane scrollback ring buffer. Holds recent PTY bytes for replay
    /// on reattach. Phase B: the reader only writes here while the pane is
    /// detached; Phase C will switch to always-on writes so pre-detach
    /// scrollback is also retained.
    pub scrollback: SharedScrollback,
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
            scrollback: Arc::new(StdMutex::new(ScrollbackRingBuffer::new(
                DEFAULT_SCROLLBACK_CAPACITY,
            ))),
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
            scrollback: Arc::new(StdMutex::new(ScrollbackRingBuffer::new(
                DEFAULT_SCROLLBACK_CAPACITY,
            ))),
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

    /// Build a `Detached` target with a `NetworkDetach`-only reason and
    /// `owner = None` (system origin), matching the daemon's pre-attach state.
    fn detached_system_target() -> SharedOutputTarget {
        Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }))
    }

    /// TS-12: detached + visible -> stays Detached.
    #[test]
    fn test_evaluate_output_target_network_detached_visible_stays_detached() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target = detached_system_target();
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, true, true, &owned_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Detached { .. }
        ));
    }

    /// TS-13: identity-scoped Connected -> Detached.
    #[test]
    fn test_evaluate_output_target_identity_scoped_connected_to_detached() {
        let (owner_tx, _rx) = mpsc::channel(16);
        let (other_tx, _other_rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(other_tx)));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        let result = evaluate_output_target(&pane, false, false, &owner_tx);
        assert!(matches!(result, EvalResult::Unchanged));
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
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::HiddenByVisibility);
                let owner = owner.as_ref().expect("owner must be set");
                assert!(owner.same_channel(&owner_tx));
            }
            _ => panic!("expected Detached"),
        }
    }

    /// TS-14: Detached -> Connected returns snapshot bytes including shadow
    /// contents and raw_passthrough. Owner = caller, reason = hidden,
    /// resolved by visible=true.
    #[test]
    fn test_evaluate_output_target_detached_to_connected_returns_snapshot() {
        let (owned_tx, _rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());
        pane.scrollback.lock().unwrap().write(b"buffered-from-ring");
        pane.shadow_parser.lock().unwrap().process(b"hello-shadow");
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

    /// F6 regression: connection A puts a pane into HiddenByVisibility
    /// (Detached, owner=A). Connection B then calls SetVisibility(true) with
    /// its own tx — must NOT reclaim the pane.
    #[test]
    fn test_evaluate_output_target_other_connection_cannot_reclaim_hidden() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let (b_tx, _b_rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(a_tx.clone()),
        }));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let result = evaluate_output_target(&pane, false, true, &b_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::HiddenByVisibility);
                let owner = owner.as_ref().expect("owner must remain A");
                assert!(
                    owner.same_channel(&a_tx),
                    "pane must still be owned by connection A"
                );
            }
            _ => panic!("expected Detached, got Connected"),
        }
    }

    /// F6: same connection's hide -> show round trip restores Connected.
    #[test]
    fn test_evaluate_output_target_same_connection_hide_show_roundtrip() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(a_tx.clone())));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let r1 = evaluate_output_target(&pane, false, false, &a_tx);
        assert!(matches!(r1, EvalResult::SwitchedToDetached));

        let r2 = evaluate_output_target(&pane, false, true, &a_tx);
        assert!(matches!(r2, EvalResult::ResumeWithSnapshot { .. }));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
    }

    /// F6: when both NetworkDetach and HiddenByVisibility are active,
    /// SetVisibility(true) only clears the hidden bit. The pane stays
    /// Detached because the network reason is still active. Only the reattach
    /// path may clear `NetworkDetach`.
    #[test]
    fn test_evaluate_output_target_both_reasons_visible_keeps_detached() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::Both,
            owner: Some(a_tx.clone()),
        }));
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let result = evaluate_output_target(&pane, false, true, &a_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, .. } => {
                assert_eq!(
                    *reason,
                    DetachReason::NetworkDetach,
                    "hidden bit cleared but network bit stays"
                );
            }
            _ => panic!("expected Detached"),
        }
    }

    /// F6: system-origin Detached (`owner = None`, reason = NetworkDetach)
    /// is NOT cleared by `evaluate_output_target` — the `NetworkDetach` bit
    /// only resolves through the reattach path. Until then, the pane stays
    /// Detached even when the caller asserts `visible = true`. The owner
    /// slot is adopted so a subsequent visibility transition is matched
    /// against the correct connection.
    #[test]
    fn test_evaluate_output_target_system_origin_stays_detached_until_reattach() {
        let (a_tx, _a_rx) = mpsc::channel(16);
        let target = detached_system_target();
        let pane = MuxPane::new_test(1, 80, 24, target.clone());

        let result = evaluate_output_target(&pane, false, true, &a_tx);
        assert!(matches!(result, EvalResult::Unchanged));
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::NetworkDetach);
                let owner = owner.as_ref().expect("owner adopted from caller");
                assert!(owner.same_channel(&a_tx));
            }
            _ => panic!("expected Detached"),
        }
    }

    /// F2: `resume_pane_with_permit` must enqueue the snapshot via the
    /// caller-supplied permit and only swap to Connected after the send.
    /// The pane mutex is held for the full sequence, so a reader thread
    /// taking the same mutex cannot push a live chunk between the two
    /// steps. This test asserts the post-conditions.
    #[tokio::test]
    async fn test_resume_pane_with_permit_sends_then_swaps() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(7, 80, 24, target.clone());
        pane.scrollback.lock().unwrap().write(b"ring-data");
        pane.shadow_parser.lock().unwrap().process(b"resume-shadow");
        pane.raw_passthrough
            .lock()
            .unwrap()
            .append(b"\x1b_Gi=7;PASS\x1b\\");

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
        assert!(matches!(outcome, ResumeOutcome::Resumed));

        // Target switched to Connected.
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));

        // Snapshot is on the channel.
        let chunk = rx.try_recv().expect("snapshot enqueued under pane lock");
        assert_eq!(chunk.pane_id, 7);
        assert!(chunk.data.starts_with(b"\x1b[H\x1b[2J"));
        let needle_passthrough = b"\x1b_Gi=7;PASS\x1b\\";
        assert!(
            chunk
                .data
                .windows(needle_passthrough.len())
                .any(|w| w == needle_passthrough),
            "snapshot must contain captured passthrough"
        );
        assert!(
            chunk
                .data
                .windows(b"ring-data".len())
                .any(|w| w == b"ring-data"),
            "snapshot must contain ring data"
        );

        // raw_passthrough drained.
        assert!(pane.raw_passthrough.lock().unwrap().is_empty());
    }

    /// F2: full Both reason cannot be cleared by `resume_pane_with_permit`
    /// alone — NetworkDetach stays. The permit is dropped without sending.
    #[tokio::test]
    async fn test_resume_pane_with_permit_keeps_detached_when_network_bit_set() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::Both,
            owner: Some(owned_tx.clone()),
        }));
        let pane = MuxPane::new_test(8, 80, 24, target.clone());

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
        assert!(matches!(outcome, ResumeOutcome::NoChange));

        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, .. } => {
                assert_eq!(*reason, DetachReason::NetworkDetach);
            }
            _ => panic!("expected Detached"),
        }
        assert!(rx.try_recv().is_err(), "no snapshot must be sent");
    }

    /// F2: connected pane is a no-op (already resumed).
    #[tokio::test]
    async fn test_resume_pane_with_permit_no_change_when_already_connected() {
        let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
        let pane = MuxPane::new_test(9, 80, 24, target.clone());

        let permit = owned_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &owned_tx, permit);
        assert!(matches!(outcome, ResumeOutcome::NoChange));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
        assert!(rx.try_recv().is_err());
    }

    /// F2: owner mismatch (different connection's tx) must be NoChange.
    #[tokio::test]
    async fn test_resume_pane_with_permit_owner_mismatch_keeps_detached() {
        let (a_tx, _a_rx) = mpsc::channel::<PtyOutputChunk>(4);
        let (b_tx, mut b_rx) = mpsc::channel::<PtyOutputChunk>(4);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::HiddenByVisibility,
            owner: Some(a_tx.clone()),
        }));
        let pane = MuxPane::new_test(10, 80, 24, target.clone());

        let permit = b_tx.reserve().await.expect("reserve permit");
        let outcome = resume_pane_with_permit(&pane, &b_tx, permit);
        assert!(matches!(outcome, ResumeOutcome::NoChange));
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Detached { .. }
        ));
        assert!(b_rx.try_recv().is_err(), "no snapshot must reach B");
    }

    #[test]
    fn test_detach_reason_combine() {
        assert_eq!(
            DetachReason::combine(
                DetachReason::NetworkDetach,
                DetachReason::HiddenByVisibility
            ),
            DetachReason::Both
        );
        assert_eq!(
            DetachReason::combine(DetachReason::NetworkDetach, DetachReason::NetworkDetach),
            DetachReason::NetworkDetach
        );
        assert_eq!(
            DetachReason::combine(DetachReason::Both, DetachReason::HiddenByVisibility),
            DetachReason::Both
        );
    }

    #[test]
    fn test_detach_reason_clear_bits() {
        assert_eq!(DetachReason::NetworkDetach.clear_network(), None);
        assert_eq!(
            DetachReason::Both.clear_network(),
            Some(DetachReason::HiddenByVisibility)
        );
        assert_eq!(
            DetachReason::HiddenByVisibility.clear_network(),
            Some(DetachReason::HiddenByVisibility)
        );
        assert_eq!(DetachReason::HiddenByVisibility.clear_hidden(), None);
        assert_eq!(
            DetachReason::Both.clear_hidden(),
            Some(DetachReason::NetworkDetach)
        );
    }
}
