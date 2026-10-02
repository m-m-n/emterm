//! Per-pane output capture state and per-pane suppression boundary record
//! (mux-snapshot-output-boundary task0001, IMPLEMENTATION.md Shared
//! Components).
//!
//! Two independent exclusions live here (IMPLEMENTATION.md D2):
//! - **The capture exclusion** ([`OutputCapture::capture`] /
//!   [`OutputCapture::captured_read`]) orders the PTY reader's shadow-parser
//!   update + scrollback-ring write against every snapshot path's read of
//!   that same state (FR2), and against [`super::MuxPane::resize`]'s ring
//!   marker + shadow resize.
//! - **The boundary exclusion** ([`OutputCapture::hold_boundary`] /
//!   [`OutputCapture::record_boundary`] / [`OutputCapture::is_boundary_covered`])
//!   orders "boundary record + snapshot insertion" against "boundary check +
//!   reader insertion" (FR11).
//!
//! The two exclusions are never held at the same time (IMPLEMENTATION.md
//! Conventions).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex as StdMutex, MutexGuard};

use tokio::sync::mpsc;

use super::output_queue::PtyOutputChunk;

/// Per-pane output capture state: the capture exclusion (guarding the last
/// assigned output sequence number) and the suppression boundary record.
///
/// Starts at "nothing captured" (`last_captured == 0`) when a pane is
/// constructed — including a pane restored by hot-upgrade (ASM-3): restore
/// builds a fresh `OutputCapture`, so a restored pane counts sequence
/// numbers from the start again.
pub struct OutputCapture {
    /// The capture exclusion. The guarded value is the last-captured output
    /// sequence number (0 = nothing captured yet).
    last_captured: StdMutex<u64>,
    /// The boundary exclusion, guarding the suppression boundary record.
    boundary: StdMutex<Vec<BoundaryEntry>>,
    /// Source of boundary-record generations (mux-suppressed-output-round3-fixes
    /// task0004, FR6/FR7). A value is handed out only while the boundary
    /// exclusion is held, so values increase in the order the records are
    /// made and are never reused — not within one destination's entry and
    /// not across destinations. 0 is never handed out.
    next_generation: AtomicU64,
    /// Test-only reader pause hooks (P1/P2/P3/P4). Zero cost in non-test
    /// builds.
    #[cfg(test)]
    pub p1: PauseHook,
    #[cfg(test)]
    pub p2: PauseHook,
    #[cfg(test)]
    pub p3: PauseHook,
    #[cfg(test)]
    pub p4: PauseHook,
}

struct BoundaryEntry {
    sender: mpsc::Sender<PtyOutputChunk>,
    boundary: u64,
    /// The incomplete construct (UTF-8 partial, awaiting designator, cut CSI)
    /// the snapshot recorded at `boundary` left the client parser in, or
    /// `None` (mux-suppressed-output-round2-fixes task0004, FR8).
    construct: Option<Vec<u8>>,
    /// Which record the entry's current values come from
    /// (mux-suppressed-output-round3-fixes task0004, FR6/FR7): a fresh value
    /// is stored whenever the boundary and construct are (re)written, so two
    /// reads of the entry that return the same generation saw the same
    /// record.
    generation: u64,
}

/// The answer of a covered query: the chunk number is covered by the
/// destination's recorded boundary, and `construct` is the incomplete
/// construct (bytes) the covering snapshot left the client parser in —
/// present only when the chunk number equals the recorded boundary
/// (mux-suppressed-output-round2-fixes task0004, FR8). `generation`
/// identifies the record the answer comes from
/// (mux-suppressed-output-round3-fixes task0004, FR6): the same value across
/// two queries means no record changed the entry in between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveredBoundary {
    pub construct: Option<Vec<u8>>,
    pub generation: u64,
}

impl OutputCapture {
    pub fn new() -> Self {
        Self {
            last_captured: StdMutex::new(0),
            boundary: StdMutex::new(Vec::new()),
            next_generation: AtomicU64::new(0),
            #[cfg(test)]
            p1: PauseHook::new(),
            #[cfg(test)]
            p2: PauseHook::new(),
            #[cfg(test)]
            p3: PauseHook::new(),
            #[cfg(test)]
            p4: PauseHook::new(),
        }
    }

    /// Run `step` under the capture exclusion, assigning it the next output
    /// sequence number (the first call gets 1) and leaving that number as
    /// the new last-captured value. `step` runs the shadow-parser update
    /// and the scrollback-ring write; the returned number is committed only
    /// together with both — a caller-supplied step that panics never
    /// updates `last_captured` (the mutex is not left poisoned either way,
    /// since every caller of this method already recovers its own internal
    /// locks via `catch_unwind`, matching the pre-existing shadow-parser
    /// panic-recovery convention).
    pub fn capture<T>(&self, step: impl FnOnce() -> T) -> (T, u64) {
        let mut last = self.last_captured.lock().unwrap();
        let result = step();
        *last += 1;
        (result, *last)
    }

    /// Run `read` under the capture exclusion WITHOUT assigning a new
    /// number. Returns the read result together with the CURRENT
    /// last-captured number, describing the same chunk set `read` observes.
    ///
    /// Used by every snapshot path (to read ring + shadow-parser state) and
    /// by [`super::MuxPane::resize`] (to run the ring marker, PTY resize and
    /// shadow resize as one step against the reader's own capture step) —
    /// resize does not correspond to an output chunk, so it never bumps the
    /// number; it only needs mutual exclusion with the reader and with
    /// concurrent snapshot reads.
    pub fn captured_read<T>(&self, read: impl FnOnce() -> T) -> (T, u64) {
        let last = self.last_captured.lock().unwrap();
        let result = read();
        (result, *last)
    }

    /// Record `boundary` for `sender`: the larger boundary is kept (an
    /// existing entry is never lowered) and, at an equal boundary, the later
    /// record wins. Every call first prunes closed senders (THREAT-MODEL
    /// TM-4), so the record holds entries only for live senders.
    pub fn record_boundary(&self, sender: &mpsc::Sender<PtyOutputChunk>, boundary: u64) {
        self.record_boundary_with_construct(sender, boundary, None);
    }

    /// [`Self::record_boundary`] together with the incomplete construct the
    /// recorded snapshot leaves the client parser in (`None` = none).
    pub fn record_boundary_with_construct(
        &self,
        sender: &mpsc::Sender<PtyOutputChunk>,
        boundary: u64,
        construct: Option<Vec<u8>>,
    ) {
        let mut entries = self.boundary.lock().unwrap();
        prune_closed(&mut entries);
        upsert_latest(
            &mut entries,
            &self.next_generation,
            sender,
            boundary,
            construct,
        );
    }

    /// Whether `number` is covered by a boundary already recorded for
    /// `sender` — "same channel" identity (`Sender::same_channel`), not a
    /// value comparison. `false` when no entry exists for `sender`.
    pub fn is_boundary_covered(&self, sender: &mpsc::Sender<PtyOutputChunk>, number: u64) -> bool {
        self.boundary_cover(sender, number).is_some()
    }

    /// [`Self::is_boundary_covered`] plus the construct: `Some` when
    /// `number` is covered, carrying the recorded construct only when
    /// `number` equals the recorded boundary.
    pub fn boundary_cover(
        &self,
        sender: &mpsc::Sender<PtyOutputChunk>,
        number: u64,
    ) -> Option<CoveredBoundary> {
        let entries = self.boundary.lock().unwrap();
        cover(&entries, sender, number)
    }

    /// Test-only: whether neither the capture exclusion nor the boundary
    /// exclusion is currently held by anyone. Lets a test prove that a piece
    /// of work runs outside both (task0004: the trailing-construct decider).
    #[cfg(test)]
    pub fn exclusions_are_free(&self) -> bool {
        self.last_captured.try_lock().is_ok() && self.boundary.try_lock().is_ok()
    }

    /// Hold the boundary exclusion for the duration of the returned guard.
    /// Lets a caller perform a covered check together with a non-blocking
    /// channel insertion (the reader), or an insertion together with a
    /// record (a snapshot path), without releasing the exclusion in
    /// between — the ordering invariant FR11 depends on.
    pub fn hold_boundary(&self) -> BoundaryGuard<'_> {
        BoundaryGuard {
            entries: self.boundary.lock().unwrap(),
            next_generation: &self.next_generation,
        }
    }
}

impl Default for OutputCapture {
    fn default() -> Self {
        Self::new()
    }
}

/// Test-only observation of the caller's lock state at the moment the
/// trailing-construct decider runs (task0004 AC-2: the decider runs outside
/// the capture exclusion and the boundary exclusion). Created by
/// [`OutputCapture::probe_decider_exclusions`].
#[cfg(test)]
pub struct DeciderProbe {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    free: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(test)]
impl DeciderProbe {
    /// Remove the hook and return `(decider calls observed, whether both
    /// exclusions were free at every one of them)`.
    pub fn finish(self) -> (usize, bool) {
        use std::sync::atomic::Ordering;
        crate::mux::snapshot_tail::ENTRY_HOOK.with(|hook| *hook.borrow_mut() = None);
        (
            self.calls.load(Ordering::SeqCst),
            self.free.load(Ordering::SeqCst),
        )
    }
}

#[cfg(test)]
impl OutputCapture {
    /// Install, on THIS thread, a hook that records whether neither
    /// exclusion of `self` is held each time the decider is entered.
    pub fn probe_decider_exclusions(self: &std::sync::Arc<Self>) -> DeciderProbe {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let free = Arc::new(AtomicBool::new(true));
        let capture = self.clone();
        let hook_calls = calls.clone();
        let hook_free = free.clone();
        crate::mux::snapshot_tail::ENTRY_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                hook_calls.fetch_add(1, Ordering::SeqCst);
                if !capture.exclusions_are_free() {
                    hook_free.store(false, Ordering::SeqCst);
                }
            }));
        });
        DeciderProbe { calls, free }
    }
}

/// A held boundary exclusion (see [`OutputCapture::hold_boundary`]).
pub struct BoundaryGuard<'a> {
    entries: MutexGuard<'a, Vec<BoundaryEntry>>,
    next_generation: &'a AtomicU64,
}

impl BoundaryGuard<'_> {
    /// Same semantics as [`OutputCapture::is_boundary_covered`], without
    /// releasing the held exclusion.
    pub fn is_covered(&self, sender: &mpsc::Sender<PtyOutputChunk>, number: u64) -> bool {
        self.cover(sender, number).is_some()
    }

    /// Same semantics as [`OutputCapture::boundary_cover`], without
    /// releasing the held exclusion: the covered decision and the construct
    /// are read under the same hold.
    pub fn cover(
        &self,
        sender: &mpsc::Sender<PtyOutputChunk>,
        number: u64,
    ) -> Option<CoveredBoundary> {
        cover(&self.entries, sender, number)
    }

    /// Same semantics as [`OutputCapture::record_boundary`], without
    /// releasing the held exclusion.
    pub fn record(&mut self, sender: &mpsc::Sender<PtyOutputChunk>, boundary: u64) {
        self.record_with_construct(sender, boundary, None);
    }

    /// Same semantics as [`OutputCapture::record_boundary_with_construct`],
    /// without releasing the held exclusion.
    pub fn record_with_construct(
        &mut self,
        sender: &mpsc::Sender<PtyOutputChunk>,
        boundary: u64,
        construct: Option<Vec<u8>>,
    ) {
        prune_closed(&mut self.entries);
        upsert_latest(
            &mut self.entries,
            self.next_generation,
            sender,
            boundary,
            construct,
        );
    }
}

fn cover(
    entries: &[BoundaryEntry],
    sender: &mpsc::Sender<PtyOutputChunk>,
    number: u64,
) -> Option<CoveredBoundary> {
    entries
        .iter()
        .find(|e| e.sender.same_channel(sender))
        .filter(|e| e.boundary >= number)
        .map(|e| CoveredBoundary {
            // The construct describes the state the snapshot left the
            // client in AFTER the boundary chunk; it says nothing about an
            // earlier covered chunk.
            construct: if e.boundary == number {
                e.construct.clone()
            } else {
                None
            },
            generation: e.generation,
        })
}

fn prune_closed(entries: &mut Vec<BoundaryEntry>) {
    entries.retain(|e| !e.sender.is_closed());
}

/// Record `(boundary, construct)` for `sender`; the caller holds the boundary
/// exclusion:
/// - no entry yet: insert, with a new generation;
/// - higher boundary: replaces boundary and construct (none included), with a
///   new generation;
/// - equal boundary: the later record wins — its construct (none included)
///   replaces the stored one, with a new generation;
/// - lower boundary: leaves the entry unchanged, generation included.
fn upsert_latest(
    entries: &mut Vec<BoundaryEntry>,
    next_generation: &AtomicU64,
    sender: &mpsc::Sender<PtyOutputChunk>,
    boundary: u64,
    construct: Option<Vec<u8>>,
) {
    if let Some(existing) = entries.iter_mut().find(|e| e.sender.same_channel(sender)) {
        if boundary >= existing.boundary {
            existing.boundary = boundary;
            existing.construct = construct;
            existing.generation = new_generation(next_generation);
        }
    } else {
        entries.push(BoundaryEntry {
            sender: sender.clone(),
            boundary,
            construct,
            generation: new_generation(next_generation),
        });
    }
}

/// The next generation value. Only called under the boundary exclusion, so
/// the values follow the order of the records; `Relaxed` suffices because the
/// exclusion itself orders every read and write of a stored generation.
fn new_generation(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::Relaxed) + 1
}

/// Test-only rendezvous point letting a test pause the PTY reader thread at
/// a specific point and release it deterministically (no sleeps).
///
/// `arm` is called from the TEST thread before the reader can reach the
/// pause point; `hit` is called from the READER thread at the pause point
/// itself. A pause point that was never armed is a no-op for `hit` (the
/// reader runs straight through), so production-shaped tests that don't
/// care about a given pause point are unaffected.
#[cfg(test)]
pub struct PauseHook {
    state: StdMutex<Option<PauseState>>,
}

#[cfg(test)]
struct PauseState {
    arrived: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

#[cfg(test)]
impl PauseHook {
    fn new() -> Self {
        Self {
            state: StdMutex::new(None),
        }
    }

    /// Arm this pause point. Returns `(arrived_rx, release_tx)`: the test
    /// blocks on `arrived_rx.recv()` to know the reader has reached the
    /// pause point, then sends on `release_tx` to let it continue.
    pub fn arm(&self) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let (arrived_tx, arrived_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        *self.state.lock().unwrap() = Some(PauseState {
            arrived: arrived_tx,
            release: release_rx,
        });
        (arrived_rx, release_tx)
    }

    /// Called from the reader thread at the pause point. If armed, signals
    /// arrival and blocks until the test releases it; otherwise returns
    /// immediately. Disarms itself on the way through, so a given `arm()`
    /// call is only ever honoured once.
    pub fn hit(&self) {
        let armed = self.state.lock().unwrap().take();
        if let Some(state) = armed {
            let _ = state.arrived.send(());
            let _ = state.release.recv();
        }
    }
}

#[cfg(test)]
impl Default for PauseHook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sender() -> (mpsc::Sender<PtyOutputChunk>, mpsc::Receiver<PtyOutputChunk>) {
        mpsc::channel(8)
    }

    #[test]
    fn capture_assigns_sequential_numbers_starting_at_one() {
        let capture = OutputCapture::new();
        let (_, n1) = capture.capture(|| ());
        let (_, n2) = capture.capture(|| ());
        let (_, n3) = capture.capture(|| ());
        assert_eq!((n1, n2, n3), (1, 2, 3));
    }

    #[test]
    fn captured_read_does_not_advance_the_number() {
        let capture = OutputCapture::new();
        capture.capture(|| ());
        capture.capture(|| ());
        let (_, seen) = capture.captured_read(|| ());
        assert_eq!(seen, 2);
        let (_, seen_again) = capture.captured_read(|| ());
        assert_eq!(seen_again, 2, "captured_read must never bump the number");
    }

    #[test]
    fn boundary_record_is_monotonic_max() {
        let capture = OutputCapture::new();
        let (tx, _rx) = sender();
        capture.record_boundary(&tx, 5);
        assert!(capture.is_boundary_covered(&tx, 5));
        assert!(!capture.is_boundary_covered(&tx, 6));
        capture.record_boundary(&tx, 3);
        assert!(
            capture.is_boundary_covered(&tx, 5),
            "a lower record must never lower an existing entry"
        );
        capture.record_boundary(&tx, 9);
        assert!(capture.is_boundary_covered(&tx, 9));
    }

    #[test]
    fn boundary_record_is_per_sender_isolated() {
        let capture = OutputCapture::new();
        let (tx_a, _rx_a) = sender();
        let (tx_b, _rx_b) = sender();
        capture.record_boundary(&tx_a, 10);
        assert!(capture.is_boundary_covered(&tx_a, 10));
        assert!(
            !capture.is_boundary_covered(&tx_b, 10),
            "a boundary recorded for A must never suppress a chunk sent to B"
        );
    }

    #[test]
    fn is_boundary_covered_false_when_no_entry_exists() {
        let capture = OutputCapture::new();
        let (tx, _rx) = sender();
        assert!(!capture.is_boundary_covered(&tx, 1));
    }

    #[test]
    fn boundary_record_prunes_closed_senders() {
        let capture = OutputCapture::new();
        let (tx_a, rx_a) = sender();
        let (tx_b, _rx_b) = sender();
        capture.record_boundary(&tx_a, 5);
        drop(rx_a);
        // Recording for a new sender prunes A's now-closed entry.
        capture.record_boundary(&tx_b, 1);
        assert!(
            !capture.is_boundary_covered(&tx_a, 5),
            "a closed sender's entry must be pruned"
        );
        assert_eq!(capture.boundary.lock().unwrap().len(), 1);
    }

    #[test]
    fn boundary_record_keeps_a_single_entry_per_sender() {
        let capture = OutputCapture::new();
        let (tx, _rx) = sender();
        capture.record_boundary(&tx, 1);
        capture.record_boundary(&tx, 2);
        capture.record_boundary(&tx, 1);
        assert_eq!(capture.boundary.lock().unwrap().len(), 1);
    }

    #[test]
    fn hold_boundary_combines_check_and_record_atomically() {
        let capture = OutputCapture::new();
        let (tx, _rx) = sender();
        let mut guard = capture.hold_boundary();
        assert!(!guard.is_covered(&tx, 1));
        guard.record(&tx, 1);
        assert!(guard.is_covered(&tx, 1));
        drop(guard);
        assert!(capture.is_boundary_covered(&tx, 1));
    }

    #[test]
    fn pause_hook_unarmed_is_a_no_op() {
        let hook = PauseHook::new();
        // Must return immediately without a release.
        hook.hit();
    }

    #[test]
    fn pause_hook_arm_blocks_hit_until_released() {
        use std::sync::Arc as StdArc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let hook = StdArc::new(PauseHook::new());
        let (arrived_rx, release_tx) = hook.arm();
        let hit_returned = StdArc::new(AtomicBool::new(false));

        let hr = hit_returned.clone();
        let h = hook.clone();
        let t = std::thread::spawn(move || {
            h.hit();
            hr.store(true, Ordering::SeqCst);
        });

        arrived_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("reader must signal arrival");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            !hit_returned.load(Ordering::SeqCst),
            "hit() must block until released"
        );
        release_tx.send(()).unwrap();
        t.join().unwrap();
        assert!(hit_returned.load(Ordering::SeqCst));
    }
}
