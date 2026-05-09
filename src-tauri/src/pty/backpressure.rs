//! PTY backpressure registry.
//!
//! Tracks bytes sent from the Rust PTY reader to the frontend that have not
//! yet been acknowledged. When the frontend stalls (e.g. WebKitGTK throttles
//! `requestAnimationFrame` after focus loss), unacked bytes accumulate. The
//! reader thread checks this counter before forwarding the next batch and
//! stalls when the high water mark is exceeded, propagating backpressure
//! through the PTY pipe buffer to the shell process.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use super::SessionId;

/// High water mark: pause reading when unacked bytes exceed this.
pub const HIGH_WATER_BYTES: usize = 8 * 1024 * 1024;
/// Low water mark: resume reading when unacked bytes drop below this.
pub const LOW_WATER_BYTES: usize = 2 * 1024 * 1024;

/// Why a `wait_for_drain_diag` call exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainExit {
    /// `in_flight` dropped to or below `LOW_WATER_BYTES` (frontend kept up).
    Drained,
    /// `set_hidden_wake` raised the hidden flag (visibility transition).
    HiddenWake,
    /// `force_wake` set `closed` (session removal).
    Closed,
    /// Mutex / condvar poison observed; treat as fatal-ish exit.
    Poisoned,
}

/// Diagnostic outcome of a `wait_for_drain_diag` call.
#[derive(Debug, Clone, Copy)]
pub struct WaitOutcome {
    /// Wall-clock time spent inside `wait_for_drain_diag`.
    pub elapsed: Duration,
    /// Why the wait exited.
    pub reason: DrainExit,
    /// Number of condvar wakeups observed before exit. Zero means we never
    /// actually parked (entered already-eligible to exit).
    pub wake_count: u32,
    /// `in_flight` snapshot taken before parking.
    pub in_flight_at_entry: usize,
    /// `in_flight` snapshot taken just before returning.
    pub in_flight_at_exit: usize,
}

/// Per-session backpressure state.
pub struct SessionBackpressure {
    /// Bytes sent to frontend but not yet acknowledged.
    in_flight: AtomicUsize,
    /// Set to true while a reader is parked in `wait_for_drain`. Allows `ack`
    /// to skip the Mutex lock + notify_all in the common case (frontend
    /// keeping up, no waiter), which otherwise pays a cross-thread sync cost
    /// per ack at 60 Hz × tab count.
    waiters: AtomicBool,
    /// One-shot wake signal raised when the session transitions to hidden.
    /// `wait_for_drain` checks this on every wake and returns immediately
    /// so the reader can re-evaluate visibility and switch to the hidden
    /// path without waiting for the next ack.
    hidden_wake: AtomicBool,
    /// Latched flag raised by `force_wake` during session removal. Once set,
    /// `wait_for_drain` exits immediately on its next check so the reader
    /// thread does not block forever on an ack the frontend will never send.
    closed: AtomicBool,
    /// Condition variable wait/notify pair. Used to wake the reader when an
    /// ack lowers the in-flight counter below LOW_WATER_BYTES.
    cond: Mutex<()>,
    cond_notify: std::sync::Condvar,
    /// Diagnostic-only: cumulative count of successful `channel.send` calls
    /// the reader thread has issued for this session. Never decreases. Read
    /// by the `pty_get_send_stats` Tauri command. E2E specs (TS-15 / TS-29)
    /// observe this counter as the source of truth that the reader stops
    /// emitting data while the session is hidden — frontend itself does not
    /// invoke `pty_get_send_stats` (FR15 撤去対象), this counter exists for
    /// E2E and on-demand manual debugging only.
    sent_count: AtomicU64,
    /// Diagnostic-only: cumulative bytes the reader thread has handed to
    /// `channel.send` for this session. Never decreases. See `sent_count`.
    sent_bytes: AtomicU64,
}

impl SessionBackpressure {
    fn new() -> Self {
        Self {
            in_flight: AtomicUsize::new(0),
            waiters: AtomicBool::new(false),
            hidden_wake: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            cond: Mutex::new(()),
            cond_notify: std::sync::Condvar::new(),
            sent_count: AtomicU64::new(0),
            sent_bytes: AtomicU64::new(0),
        }
    }

    /// Record that `n` bytes have been sent to the frontend. Updates the
    /// backpressure `in_flight` counter (subject to ack).
    pub fn add_sent(&self, n: usize) {
        self.in_flight.fetch_add(n, Ordering::AcqRel);
    }

    /// Record that the frontend has consumed `n` bytes.
    pub fn ack(&self, n: usize) {
        // Saturating subtract to tolerate any double-ack or over-ack from the
        // frontend without panicking.
        let mut cur = self.in_flight.load(Ordering::Acquire);
        loop {
            let new = cur.saturating_sub(n);
            match self.in_flight.compare_exchange_weak(
                cur,
                new,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
        // Wake any reader waiting for drain — but only if one is actually
        // parked. The common case is no waiter: skipping the Mutex acquire
        // and notify_all keeps `pty_ack` lock-free at 60 Hz.
        if self.waiters.load(Ordering::Acquire) {
            let _guard = self.cond.lock().expect("backpressure cond poisoned");
            self.cond_notify.notify_all();
        }
    }

    /// Wakes any parked `wait_for_drain` waiter and marks the session as
    /// closed so the waiter exits immediately on next check. Used during
    /// session removal.
    pub fn force_wake(&self) {
        self.closed.store(true, Ordering::Release);
        if self.waiters.load(Ordering::Acquire) {
            let _guard = self.cond.lock().expect("backpressure cond poisoned");
            self.cond_notify.notify_all();
        }
    }

    /// Raise the hidden wake flag and unpark any reader sitting in
    /// `wait_for_drain`. Called from `pty_set_visibility` when the session
    /// transitions to hidden so the reader stops waiting on acks the
    /// frontend will not send while hidden.
    pub fn set_hidden_wake(&self) {
        self.hidden_wake.store(true, Ordering::Release);
        if self.waiters.load(Ordering::Acquire) {
            let _guard = self.cond.lock().expect("backpressure cond poisoned");
            self.cond_notify.notify_all();
        }
    }

    /// Block until in-flight bytes drop below `LOW_WATER_BYTES`, the
    /// `hidden_wake` flag is raised, or the session is closed via
    /// `force_wake`. Returns the time spent waiting.
    ///
    /// No timeout: callers depend on either an ack from the frontend
    /// (visible path), a hidden-wake signal (visibility transition), or
    /// session removal (`force_wake` sets `closed`) to resume.
    pub fn wait_for_drain(&self) -> Duration {
        self.wait_for_drain_diag().elapsed
    }

    /// Diagnostic variant of `wait_for_drain` that exposes the exit reason
    /// and how many condvar wakeups occurred before exit. The wake count
    /// distinguishes spurious wakes / repeated ack-then-still-over-low-water
    /// cycles from a single direct unpark, which helps explain stalls where
    /// `before == after` in_flight (no progress despite many notifications).
    pub fn wait_for_drain_diag(&self) -> WaitOutcome {
        let start = std::time::Instant::now();
        let in_flight_at_entry = self.in_flight.load(Ordering::Acquire);
        let mut guard = match self.cond.lock() {
            Ok(g) => g,
            Err(_) => {
                return WaitOutcome {
                    elapsed: start.elapsed(),
                    reason: DrainExit::Poisoned,
                    wake_count: 0,
                    in_flight_at_entry,
                    in_flight_at_exit: self.in_flight.load(Ordering::Acquire),
                };
            }
        };
        self.waiters.store(true, Ordering::Release);
        let mut wake_count: u32 = 0;
        let reason: DrainExit = loop {
            // Check exit conditions in order so the reason reflects what
            // actually unblocked us.
            if self.closed.load(Ordering::Acquire) {
                break DrainExit::Closed;
            }
            if self.hidden_wake.swap(false, Ordering::AcqRel) {
                break DrainExit::HiddenWake;
            }
            if self.in_flight.load(Ordering::Acquire) <= LOW_WATER_BYTES {
                break DrainExit::Drained;
            }
            match self.cond_notify.wait(guard) {
                Ok(g) => {
                    guard = g;
                    wake_count = wake_count.saturating_add(1);
                }
                Err(_) => {
                    self.waiters.store(false, Ordering::Release);
                    return WaitOutcome {
                        elapsed: start.elapsed(),
                        reason: DrainExit::Poisoned,
                        wake_count,
                        in_flight_at_entry,
                        in_flight_at_exit: self.in_flight.load(Ordering::Acquire),
                    };
                }
            }
        };
        self.waiters.store(false, Ordering::Release);
        WaitOutcome {
            elapsed: start.elapsed(),
            reason,
            wake_count,
            in_flight_at_entry,
            in_flight_at_exit: self.in_flight.load(Ordering::Acquire),
        }
    }

    /// Returns the current unacked byte count.
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Returns true if the in-flight counter is over the high water mark.
    pub fn over_high_water(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) > HIGH_WATER_BYTES
    }

    /// Diagnostic-only: record one successful `channel.send` of `n` bytes.
    /// Called by the reader thread only on the visible (non-hidden) path.
    /// Counters never decrease and are exposed via `sent_count` / `sent_bytes`
    /// for E2E specs (TS-15 / TS-29) which assert that the reader stops
    /// emitting data while the session is hidden. This is the source of truth
    /// for that assertion — frontend itself does not invoke
    /// `pty_get_send_stats` (FR15 撤去対象).
    pub fn record_send_success(&self, n: usize) {
        self.sent_count.fetch_add(1, Ordering::Relaxed);
        self.sent_bytes.fetch_add(n as u64, Ordering::Relaxed);
    }

    /// Diagnostic-only getter for the cumulative `channel.send` call count.
    pub fn sent_count(&self) -> u64 {
        self.sent_count.load(Ordering::Relaxed)
    }

    /// Diagnostic-only getter for the cumulative bytes sent via `channel.send`.
    pub fn sent_bytes(&self) -> u64 {
        self.sent_bytes.load(Ordering::Relaxed)
    }
}

/// Registry of per-session backpressure state.
#[derive(Clone)]
pub struct BackpressureRegistry {
    inner: Arc<RwLock<HashMap<SessionId, Arc<SessionBackpressure>>>>,
}

impl Default for BackpressureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BackpressureRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new session. Returns the existing entry if already present.
    pub fn register(&self, id: SessionId) -> Arc<SessionBackpressure> {
        let mut guard = self.inner.write().expect("backpressure registry poisoned");
        guard
            .entry(id)
            .or_insert_with(|| Arc::new(SessionBackpressure::new()))
            .clone()
    }

    /// Look up an existing session entry.
    pub fn get(&self, id: &str) -> Option<Arc<SessionBackpressure>> {
        let guard = self.inner.read().expect("backpressure registry poisoned");
        guard.get(id).cloned()
    }

    /// Remove a session entry. Wakes any parked reader so it can exit
    /// promptly instead of blocking forever on an ack that will never
    /// arrive.
    pub fn remove(&self, id: &str) -> Option<Arc<SessionBackpressure>> {
        let mut guard = self.inner.write().expect("backpressure registry poisoned");
        let entry = guard.remove(id);
        if let Some(bp) = entry.as_ref() {
            bp.force_wake();
        }
        entry
    }

    /// Snapshot the current in-flight byte counts for every registered
    /// session. Used by the reader-thread stalled diagnostic so a single
    /// `backpressure stalled` warn line can show whether other sessions are
    /// also backed up (Claude Code parallel-output scenario) or whether
    /// only the reporting session is the offender. The returned vector is
    /// unsorted; callers may sort by count if they want a top-N display.
    pub fn snapshot_in_flight(&self) -> Vec<(SessionId, usize)> {
        let guard = self.inner.read().expect("backpressure registry poisoned");
        guard
            .iter()
            .map(|(id, bp)| (id.clone(), bp.in_flight()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_ack() {
        let bp = SessionBackpressure::new();
        bp.add_sent(1000);
        assert_eq!(bp.in_flight(), 1000);
        bp.ack(400);
        assert_eq!(bp.in_flight(), 600);
        bp.ack(1_000); // Over-ack
        assert_eq!(bp.in_flight(), 0);
    }

    #[test]
    fn test_high_water_threshold() {
        let bp = SessionBackpressure::new();
        bp.add_sent(HIGH_WATER_BYTES + 1);
        assert!(bp.over_high_water());
        bp.ack(2);
        assert!(!bp.over_high_water());
    }

    #[test]
    fn test_registry_register_and_get() {
        let reg = BackpressureRegistry::new();
        let bp = reg.register("session-1".to_string());
        bp.add_sent(500);
        let got = reg.get("session-1").expect("must exist");
        assert_eq!(got.in_flight(), 500);
    }

    #[test]
    fn test_registry_remove() {
        let reg = BackpressureRegistry::new();
        reg.register("session-1".to_string());
        assert!(reg.remove("session-1").is_some());
        assert!(reg.get("session-1").is_none());
    }

    #[test]
    fn test_wait_for_drain_returns_quickly_when_under_water() {
        let bp = Arc::new(SessionBackpressure::new());
        bp.add_sent(LOW_WATER_BYTES / 2);
        // Under low water -> wait_for_drain must return without blocking.
        let waited = bp.wait_for_drain();
        assert!(waited < Duration::from_millis(50));
    }

    #[test]
    fn test_wait_for_drain_wakes_on_ack() {
        let bp = Arc::new(SessionBackpressure::new());
        bp.add_sent(HIGH_WATER_BYTES + LOW_WATER_BYTES);
        let bp_clone = bp.clone();
        let handle = std::thread::spawn(move || bp_clone.wait_for_drain());
        std::thread::sleep(Duration::from_millis(20));
        bp.ack(HIGH_WATER_BYTES + LOW_WATER_BYTES);
        let waited = handle.join().expect("thread panicked");
        // ack-driven wake must return promptly (no fixed timeout).
        assert!(waited < Duration::from_secs(1));
    }

    #[test]
    fn test_wait_for_drain_wakes_on_set_hidden_wake() {
        let bp = Arc::new(SessionBackpressure::new());
        bp.add_sent(HIGH_WATER_BYTES + LOW_WATER_BYTES);
        let bp_clone = bp.clone();
        let handle = std::thread::spawn(move || bp_clone.wait_for_drain());
        std::thread::sleep(Duration::from_millis(20));
        // Hidden transition must wake the parked reader without an ack.
        bp.set_hidden_wake();
        let waited = handle.join().expect("thread panicked");
        assert!(waited < Duration::from_secs(1));
        // in_flight must NOT be cleared by hidden wake.
        assert_eq!(bp.in_flight(), HIGH_WATER_BYTES + LOW_WATER_BYTES);
    }

    #[test]
    fn test_wait_for_drain_force_wake_returns() {
        let bp = Arc::new(SessionBackpressure::new());
        bp.add_sent(HIGH_WATER_BYTES + LOW_WATER_BYTES);
        let bp_clone = bp.clone();
        let handle = std::thread::spawn(move || bp_clone.wait_for_drain());
        std::thread::sleep(Duration::from_millis(50));
        bp.force_wake();
        let waited = handle.join().expect("thread panicked");
        assert!(
            waited < Duration::from_secs(5),
            "wait_for_drain did not release after force_wake (waited {:?})",
            waited
        );
    }
}
