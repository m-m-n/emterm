//! PTY backpressure registry.
//!
//! Tracks bytes sent from the Rust PTY reader to the frontend that have not
//! yet been acknowledged. When the frontend stalls (e.g. WebKitGTK throttles
//! `requestAnimationFrame` after focus loss), unacked bytes accumulate. The
//! reader thread checks this counter before forwarding the next batch and
//! stalls when the high water mark is exceeded, propagating backpressure
//! through the PTY pipe buffer to the shell process.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use super::SessionId;

/// High water mark: pause reading when unacked bytes exceed this.
pub const HIGH_WATER_BYTES: usize = 8 * 1024 * 1024;
/// Low water mark: resume reading when unacked bytes drop below this.
pub const LOW_WATER_BYTES: usize = 2 * 1024 * 1024;
/// Maximum time to wait for the frontend to drain before forwarding anyway.
/// Prevents permanent stall if the frontend died without cleanup.
pub const MAX_BACKPRESSURE_WAIT: Duration = Duration::from_secs(60);
/// Sleep step while waiting for drain. Short enough to recover quickly.
pub const BACKPRESSURE_POLL: Duration = Duration::from_millis(50);

/// Per-session backpressure state.
pub struct SessionBackpressure {
    /// Bytes sent to frontend but not yet acknowledged.
    in_flight: AtomicUsize,
    /// Set to true while a reader is parked in `wait_for_drain`. Allows `ack`
    /// to skip the Mutex lock + notify_all in the common case (frontend
    /// keeping up, no waiter), which otherwise pays a cross-thread sync cost
    /// per ack at 60 Hz × tab count.
    waiters: AtomicBool,
    /// Condition variable wait/notify pair. Used to wake the reader when an
    /// ack lowers the in-flight counter below LOW_WATER_BYTES.
    cond: Mutex<()>,
    cond_notify: std::sync::Condvar,
}

impl SessionBackpressure {
    fn new() -> Self {
        Self {
            in_flight: AtomicUsize::new(0),
            waiters: AtomicBool::new(false),
            cond: Mutex::new(()),
            cond_notify: std::sync::Condvar::new(),
        }
    }

    /// Record that `n` bytes have been sent to the frontend.
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

    /// Wake any parked reader. Used during session removal so the reader
    /// thread doesn't sit in `wait_for_drain` for up to MAX_BACKPRESSURE_WAIT
    /// when no further acks can possibly arrive.
    pub fn force_wake(&self) {
        if self.waiters.load(Ordering::Acquire) {
            let _guard = self.cond.lock().expect("backpressure cond poisoned");
            self.cond_notify.notify_all();
        }
    }

    /// Block until in-flight bytes drop below `LOW_WATER_BYTES` or the wait
    /// timeout elapses. Returns the time spent waiting.
    pub fn wait_for_drain(&self) -> Duration {
        let start = std::time::Instant::now();
        let mut guard = match self.cond.lock() {
            Ok(g) => g,
            Err(_) => return start.elapsed(),
        };
        // Mark presence while parked so `ack` knows to wake us. Cleared in
        // every exit path (timeout, condition met, error).
        self.waiters.store(true, Ordering::Release);
        while self.in_flight.load(Ordering::Acquire) > LOW_WATER_BYTES
            && start.elapsed() < MAX_BACKPRESSURE_WAIT
        {
            let remaining = MAX_BACKPRESSURE_WAIT.saturating_sub(start.elapsed());
            let timeout = remaining.min(BACKPRESSURE_POLL);
            let res = self.cond_notify.wait_timeout(guard, timeout);
            match res {
                Ok((g, _wait_result)) => guard = g,
                Err(_) => {
                    self.waiters.store(false, Ordering::Release);
                    return start.elapsed();
                }
            }
        }
        self.waiters.store(false, Ordering::Release);
        start.elapsed()
    }

    /// Returns the current unacked byte count.
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Returns true if the in-flight counter is over the high water mark.
    pub fn over_high_water(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) > HIGH_WATER_BYTES
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
    /// promptly instead of waiting up to MAX_BACKPRESSURE_WAIT for an ack
    /// that will never arrive.
    pub fn remove(&self, id: &str) -> Option<Arc<SessionBackpressure>> {
        let mut guard = self.inner.write().expect("backpressure registry poisoned");
        let entry = guard.remove(id);
        if let Some(bp) = entry.as_ref() {
            bp.force_wake();
        }
        entry
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
        let waited = bp.wait_for_drain();
        assert!(waited < Duration::from_millis(50));
    }

    #[test]
    fn test_wait_for_drain_wakes_on_ack() {
        let bp = Arc::new(SessionBackpressure::new());
        bp.add_sent(HIGH_WATER_BYTES + LOW_WATER_BYTES);
        let bp_clone = bp.clone();
        let handle = std::thread::spawn(move || bp_clone.wait_for_drain());
        // Give the waiter a moment to enter the cond wait.
        std::thread::sleep(Duration::from_millis(20));
        bp.ack(HIGH_WATER_BYTES + LOW_WATER_BYTES);
        let waited = handle.join().expect("thread panicked");
        assert!(waited < Duration::from_secs(1));
    }
}
