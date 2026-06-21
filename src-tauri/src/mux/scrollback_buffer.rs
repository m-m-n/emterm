//! Per-pane circular scrollback buffer.
//!
//! Held by `MuxPane` for the lifetime of the pane. PTY output flowing
//! through the daemon is appended here so that on reattach the daemon can
//! replay recent bytes to the freshly-attached client.
//!
//! In Phase B this buffer is still written only while the pane is detached
//! (matching the previous detach-only behavior); Phase C will switch the
//! reader to write unconditionally so that pre-detach scrollback is also
//! retained.
//!
//! The buffer has a configurable capacity (default 2 MiB) and overwrites
//! oldest data when full.

/// Default scrollback capacity: 2 MiB per pane.
///
/// At ~206 columns this holds roughly 10,000 lines of scrollback worth of
/// raw bytes. The cap keeps daemon memory predictable (pane_count × 2 MiB)
/// — previously 64 MiB caused a 320 MiB spike across five detached panes.
pub const DEFAULT_SCROLLBACK_CAPACITY: usize = 2 * 1024 * 1024;

/// Circular byte buffer with fixed capacity.
pub struct ScrollbackRingBuffer {
    buf: Vec<u8>,
    capacity: usize,
    write_pos: usize,
    len: usize,
}

impl ScrollbackRingBuffer {
    /// Create a new ring buffer with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            capacity,
            write_pos: 0,
            len: 0,
        }
    }

    /// Write data to the ring buffer. Overwrites oldest data if full.
    pub fn write(&mut self, data: &[u8]) {
        if data.len() >= self.capacity {
            // Data larger than buffer: keep only the tail
            let start = data.len() - self.capacity;
            self.buf.copy_from_slice(&data[start..]);
            self.write_pos = 0;
            self.len = self.capacity;
            return;
        }

        let end_space = self.capacity - self.write_pos;
        if data.len() <= end_space {
            self.buf[self.write_pos..self.write_pos + data.len()].copy_from_slice(data);
        } else {
            // Wrap around
            self.buf[self.write_pos..].copy_from_slice(&data[..end_space]);
            let remaining = data.len() - end_space;
            self.buf[..remaining].copy_from_slice(&data[end_space..]);
        }

        self.write_pos = (self.write_pos + data.len()) % self.capacity;
        self.len = (self.len + data.len()).min(self.capacity);
    }

    /// Read all accumulated data in chronological order.
    /// Returns a Vec containing the buffer contents from oldest to newest.
    pub fn read_all(&self) -> Vec<u8> {
        if self.len == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(self.len);
        if self.len < self.capacity {
            // Buffer hasn't wrapped: data starts at (write_pos - len)
            let start = self.write_pos.wrapping_sub(self.len) % self.capacity;
            if start < self.write_pos {
                result.extend_from_slice(&self.buf[start..self.write_pos]);
            } else {
                result.extend_from_slice(&self.buf[start..]);
                result.extend_from_slice(&self.buf[..self.write_pos]);
            }
        } else {
            // Buffer is full: read_pos == write_pos
            result.extend_from_slice(&self.buf[self.write_pos..]);
            result.extend_from_slice(&self.buf[..self.write_pos]);
        }

        result
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.write_pos = 0;
        self.len = 0;
    }

    /// Current data length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Buffer capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buffer() {
        let rb = ScrollbackRingBuffer::new(1024);
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
        assert_eq!(rb.read_all(), Vec::<u8>::new());
    }

    #[test]
    fn test_simple_write_read() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write(b"hello");
        assert_eq!(rb.len(), 5);
        assert_eq!(rb.read_all(), b"hello");
    }

    #[test]
    fn test_multiple_writes() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write(b"hello ");
        rb.write(b"world");
        assert_eq!(rb.len(), 11);
        assert_eq!(rb.read_all(), b"hello world");
    }

    #[test]
    fn test_wrap_around() {
        let mut rb = ScrollbackRingBuffer::new(8);
        rb.write(b"ABCDEF"); // 6 bytes, pos=6
        rb.write(b"GHI"); // wraps: pos=1, overwrites first byte
        assert_eq!(rb.len(), 8); // capped at capacity
        assert_eq!(rb.read_all(), b"BCDEFGHI");
    }

    #[test]
    fn test_overflow_large_write() {
        let mut rb = ScrollbackRingBuffer::new(4);
        rb.write(b"ABCDEFGH"); // larger than capacity, keeps last 4
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.read_all(), b"EFGH");
    }

    #[test]
    fn test_exact_capacity() {
        let mut rb = ScrollbackRingBuffer::new(4);
        rb.write(b"ABCD");
        assert_eq!(rb.len(), 4);
        assert_eq!(rb.read_all(), b"ABCD");
    }

    #[test]
    fn test_clear() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write(b"data");
        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.read_all(), Vec::<u8>::new());
    }

    #[test]
    fn test_write_after_clear() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write(b"old data");
        rb.clear();
        rb.write(b"new data");
        assert_eq!(rb.read_all(), b"new data");
    }

    #[test]
    fn test_capacity() {
        let rb = ScrollbackRingBuffer::new(4096);
        assert_eq!(rb.capacity(), 4096);
    }

    /// Perf bench: time `read_all()` on a 2 MiB ring filled past capacity (so
    /// the wrap-around branch is exercised). This runs once per mux snapshot
    /// rebuild — `handle_request_pane_snapshot` / `reattach::build_snapshot_bytes`
    /// call it on the full pane scrollback every tab switch.
    ///
    /// Gated `#[ignore]`. Invoke with:
    ///
    /// ```sh
    /// CARGO_TARGET_DIR=src-tauri/target cargo test --release \
    ///   --manifest-path src-tauri/Cargo.toml --lib --features gui \
    ///   scrollback_read_all_bench_2mib_wrapped \
    ///   -- --nocapture --include-ignored
    /// ```
    #[test]
    #[ignore]
    fn scrollback_read_all_bench_2mib_wrapped() {
        use std::time::Instant;
        let cap = 2 * 1024 * 1024;
        let mut rb = ScrollbackRingBuffer::new(cap);
        // Fill 3x capacity in 4 KiB chunks so the ring wraps and write_pos
        // lands somewhere in the middle (mimics a long-running pane).
        let chunk = vec![b'x'; 4096];
        let total_writes = (cap * 3) / chunk.len();
        for _ in 0..total_writes {
            rb.write(&chunk);
        }
        // Warm-up.
        for _ in 0..2 {
            let _ = rb.read_all();
        }
        let iters = 50;
        let start = Instant::now();
        for _ in 0..iters {
            let out = rb.read_all();
            std::hint::black_box(out);
        }
        let elapsed = start.elapsed();
        let per = elapsed / iters as u32;
        eprintln!(
            "[bench] ScrollbackRingBuffer::read_all 2MiB wrapped: {iters} iters / {:?} → {:?}/call ({:.1} MiB/s)",
            elapsed,
            per,
            (2.0 * iters as f64) / elapsed.as_secs_f64(),
        );
        // SPEC.md "Performance Goals" (FR5): a plain Vec copy of 2 MiB
        // must stay under 1 ms — if this regresses, the daemon-side ring
        // read has become a bottleneck of its own.
        let threshold = std::time::Duration::from_millis(1);
        assert!(
            per < threshold,
            "ScrollbackRingBuffer::read_all per-call {:?} ≥ threshold {:?} (FR5)",
            per,
            threshold,
        );
    }

    #[test]
    fn test_repeated_small_writes_overflow() {
        let mut rb = ScrollbackRingBuffer::new(8);
        for i in 0..20u8 {
            rb.write(&[i]);
        }
        assert_eq!(rb.len(), 8);
        // Last 8 bytes: 12,13,14,15,16,17,18,19
        assert_eq!(rb.read_all(), vec![12, 13, 14, 15, 16, 17, 18, 19]);
    }
}
