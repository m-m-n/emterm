//! Bounded byte buffer for raw passthrough sequences (Kitty / SIXEL / OSC 9999).
//!
//! Used by the mux daemon to retain replayable image / Markdown bytes seen
//! while a pane is detached, so reattach can resurrect the visual state. The
//! frontend-side `SessionVisibilityState` from the legacy Tauri build has been
//! dropped; native-poc tracks visibility through the per-tab PTY pause flag
//! and `term_core` state, not through a registry of `vt100::Parser`s.

use std::collections::VecDeque;

/// Default raw passthrough capacity for non-mux sessions (4 MiB). Retained
/// for parity with the legacy build even though native-poc itself does not
/// drive a per-session passthrough buffer outside the daemon.
pub const HIDDEN_PASSTHROUGH_CAPACITY_NONMUX: usize = 4 * 1024 * 1024;

/// Default raw passthrough capacity for mux panes (1 MiB per pane).
pub const HIDDEN_PASSTHROUGH_CAPACITY_MUX: usize = 1024 * 1024;

/// Maximum partial-buffer size while a passthrough scanner is mid-sequence.
/// Beyond this, the in-flight sequence is dropped (with a warn).
pub const PARTIAL_SEQUENCE_MAX: usize = 16 * 1024 * 1024;

/// Bounded byte-buffer that retains the *most recent* `capacity` bytes
/// of raw passthrough sequences (Kitty / SIXEL / OSC 9999) seen while
/// hidden. Old bytes are evicted from the front when capacity is exceeded.
///
/// `append` returns `true` whenever the call caused bytes to be dropped,
/// so the caller can emit a single warn per drop episode.
pub struct RawPassthroughBuffer {
    capacity: usize,
    buf: VecDeque<u8>,
    drop_warned: bool,
}

impl RawPassthroughBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buf: VecDeque::new(),
            drop_warned: false,
        }
    }

    /// Append `data` to the buffer, evicting from the front if capacity is
    /// exceeded. Returns `true` if a drop happened *and* this is the first
    /// drop since the last `clear()` (so the caller knows to warn once).
    pub fn append(&mut self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        if data.len() > self.capacity {
            self.buf.clear();
            let start = data.len() - self.capacity;
            self.buf.extend(data[start..].iter().copied());
            let first = !self.drop_warned;
            self.drop_warned = true;
            return first;
        }
        let new_len = self.buf.len() + data.len();
        if new_len > self.capacity {
            let drop_n = new_len - self.capacity;
            self.buf.drain(..drop_n);
            self.buf.extend(data.iter().copied());
            let first = !self.drop_warned;
            self.drop_warned = true;
            return first;
        }
        self.buf.extend(data.iter().copied());
        false
    }

    pub fn read_all(&self) -> Vec<u8> {
        Vec::from_iter(self.buf.iter().copied())
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.drop_warned = false;
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_passthrough_keeps_tail_when_over_capacity() {
        let mut buf = RawPassthroughBuffer::new(8);
        assert!(!buf.append(b"abcd"));
        assert!(!buf.append(b"efgh"));
        assert!(buf.append(b"ijkl"), "first overflow must report drop");
        assert_eq!(buf.read_all(), b"efghijkl");
    }

    #[test]
    fn raw_passthrough_warn_only_once() {
        let mut buf = RawPassthroughBuffer::new(4);
        buf.append(b"abcd");
        let first = buf.append(b"e");
        let second = buf.append(b"f");
        assert!(first, "first overflow reports drop");
        assert!(!second, "subsequent overflow does not re-report");
    }

    #[test]
    fn raw_passthrough_clear_resets_warn_latch() {
        let mut buf = RawPassthroughBuffer::new(4);
        buf.append(b"abcd");
        buf.append(b"e");
        buf.clear();
        let first_after_clear = buf.append(b"abcde");
        assert!(first_after_clear, "clear() resets the warn latch");
    }

    #[test]
    fn raw_passthrough_giant_chunk_keeps_tail() {
        let mut buf = RawPassthroughBuffer::new(4);
        assert!(buf.append(b"abcdefgh"));
        assert_eq!(buf.read_all(), b"efgh");
    }
}
