//! Fixed-capacity drop-oldest ring buffer for PTY pause-and-replay.
//!
//! When a tab is attached to a mux session, the underlying native PTY reader
//! is **paused** — its bytes flow into a ring buffer instead of through
//! `term_core`. On detach, the ring buffer is drained into `term_core` so
//! no output is lost.
//!
//! Capacity is fixed (256 KiB by default — see [`DEFAULT_CAPACITY`]). When
//! the buffer is full the oldest byte is dropped on each push (`overflowed`
//! latches `true` so the caller can log + report the loss).
//!
//! Independent of `std::io::Read`/`Write` — we only need a byte queue, and
//! callers always push complete chunks read from the PTY.

use std::collections::VecDeque;

/// Default capacity (256 KiB). The chosen size mirrors the
/// `RING_BUFFER_DEFAULT_KIB` constant in `doc/tasks/.../IMPLEMENTATION.md`.
pub const DEFAULT_CAPACITY: usize = 256 * 1024;

/// Drop-oldest ring buffer over `u8`.
#[derive(Debug)]
pub struct RingBuffer {
    inner: VecDeque<u8>,
    capacity: usize,
    /// Latches `true` if at least one byte was dropped due to overflow.
    /// Cleared by [`drain`].
    overflowed: bool,
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl RingBuffer {
    /// New buffer with `capacity` bytes.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: VecDeque::with_capacity(capacity),
            capacity,
            overflowed: false,
        }
    }

    /// Current number of buffered bytes.
    #[allow(dead_code)] // exposed for diagnostics + future status-bar widget.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Buffer is empty.
    #[allow(dead_code)] // used by tests; future status bar widget will read it.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Maximum size (constructor-fixed).
    #[allow(dead_code)] // exposed for diagnostics; future settings UI will read it.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// True if the buffer dropped at least one byte since the last drain.
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Append `bytes`. When the resulting size exceeds capacity, the oldest
    /// bytes are dropped (FIFO) until the buffer fits again. Sets
    /// `overflowed` if any byte was dropped.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.capacity == 0 {
            // Pathological case — every byte is dropped immediately.
            if !bytes.is_empty() {
                self.overflowed = true;
            }
            return;
        }

        let incoming = bytes.len();
        if incoming > self.capacity {
            // Fast path: only the trailing `capacity` bytes can survive,
            // and we know we are overflowing.
            self.inner.clear();
            self.inner.extend(&bytes[incoming - self.capacity..]);
            self.overflowed = true;
            return;
        }
        if incoming == self.capacity && incoming > 0 {
            // Exactly fills the buffer — discard old contents and copy in.
            // Only counts as an overflow when there was something to drop.
            let was_overflow = !self.inner.is_empty();
            self.inner.clear();
            self.inner.extend(bytes);
            if was_overflow {
                self.overflowed = true;
            }
            return;
        }

        // Make room for incoming bytes.
        let after = self.inner.len() + incoming;
        if after > self.capacity {
            let drop_n = after - self.capacity;
            self.inner.drain(..drop_n);
            self.overflowed = true;
        }
        self.inner.extend(bytes);
    }

    /// Drain the buffer into a fresh `Vec<u8>`. Resets `overflowed`. Callers
    /// pass the returned bytes to `term_core::process_pty_data` (or the
    /// mux-mode equivalent) on resume.
    pub fn drain(&mut self) -> Vec<u8> {
        let out: Vec<u8> = self.inner.drain(..).collect();
        self.overflowed = false;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_buffer_is_empty() {
        let r = RingBuffer::default();
        assert_eq!(r.len(), 0);
        assert!(r.is_empty());
        assert!(!r.overflowed());
        assert_eq!(r.capacity(), DEFAULT_CAPACITY);
    }

    #[test]
    fn push_below_capacity_preserves_all_bytes() {
        let mut r = RingBuffer::with_capacity(8);
        r.push(b"abc");
        r.push(b"de");
        let out = r.drain();
        assert_eq!(out, b"abcde");
        assert!(!r.overflowed());
    }

    #[test]
    fn push_at_capacity_keeps_everything() {
        let mut r = RingBuffer::with_capacity(4);
        r.push(b"abcd");
        assert!(!r.overflowed());
        let out = r.drain();
        assert_eq!(out, b"abcd");
    }

    #[test]
    fn push_over_capacity_drops_oldest() {
        let mut r = RingBuffer::with_capacity(4);
        r.push(b"abcd");
        r.push(b"ef"); // drops 'a','b'; result = "cdef"
        assert!(r.overflowed());
        let out = r.drain();
        assert_eq!(out, b"cdef");
    }

    #[test]
    fn single_oversized_push_keeps_trailing_capacity() {
        let mut r = RingBuffer::with_capacity(4);
        r.push(b"ABCDEFGH"); // only the last 4 bytes survive
        assert!(r.overflowed());
        let out = r.drain();
        assert_eq!(out, b"EFGH");
    }

    #[test]
    fn drain_resets_overflowed_flag() {
        let mut r = RingBuffer::with_capacity(2);
        r.push(b"abc");
        assert!(r.overflowed());
        r.drain();
        assert!(!r.overflowed());
    }

    #[test]
    fn zero_capacity_drops_everything_and_overflows() {
        let mut r = RingBuffer::with_capacity(0);
        r.push(b"xyz");
        assert_eq!(r.len(), 0);
        assert!(r.overflowed());
    }

    #[test]
    fn empty_push_to_zero_capacity_is_noop() {
        let mut r = RingBuffer::with_capacity(0);
        r.push(b"");
        assert!(!r.overflowed());
    }
}
