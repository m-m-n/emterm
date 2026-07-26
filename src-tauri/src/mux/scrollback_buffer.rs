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

use std::collections::VecDeque;

/// Default scrollback capacity: 2 MiB per pane.
///
/// At ~206 columns this holds roughly 10,000 lines of scrollback worth of
/// raw bytes. The cap keeps daemon memory predictable (pane_count × 2 MiB)
/// — previously 64 MiB caused a 320 MiB spike across five detached panes.
pub const DEFAULT_SCROLLBACK_CAPACITY: usize = 2 * 1024 * 1024;

/// `dim_markers` is the SOLE authority for "which dimensions were in effect
/// at a given byte offset" (task0003 D1, review round-2 findings
/// `a6ab9b340119beed` / `15c54fb74bb91ec7`) — resize dimensions are NEVER
/// written into `buf` at all — [`Self::write_resize_marker`] only records
/// `(offset, cols, rows)` here.
///
/// task0004 round-4 rework (D1'): this structural side channel is now ALSO
/// how dimensions leave the ring — [`Self::read_segments`] exposes the
/// entries directly (offset-adjusted for the retained window), and
/// [`Self::read_all`] returns plain content bytes with no marker synthesis
/// at all. Rounds 1-3 additionally synthesized fresh `OSC 777;emterm;resize;
/// …` marker BYTES into `read_all`'s output so the wire payload carried
/// dimensions in-band; every one of that design's residual forgery findings
/// (`95fb7c115b0b64da`, `4a22bd439fcdaf56`, `d4a83d5403bf1d7c`) traced back
/// to that byte-shaped representation being — definitionally — also
/// forgeable by PTY output. Carrying `dim_markers` entries structurally all
/// the way to the wire (via [`Self::read_segments`] and
/// `mux_ipc::protocol::DimSegment`) closes the class outright: there is no
/// longer any marker-shaped byte sequence for PTY output to collide with.
pub struct ScrollbackRingBuffer {
    buf: Vec<u8>,
    capacity: usize,
    write_pos: usize,
    len: usize,
    /// Cumulative count of bytes ever passed to [`Self::write`], regardless
    /// of eviction. Monotonically increasing; used to compute the absolute
    /// stream offset of the oldest byte currently retained
    /// (`total_written.saturating_sub(capacity)`). Resize markers no longer
    /// consume any of this budget (task0003 D1) — only real content bytes
    /// advance it.
    total_written: u64,
    /// `(offset, cols, rows)` for every resize marker recorded via
    /// [`Self::write_resize_marker`], in increasing `offset` order.
    /// `offset` is the absolute content-stream position (`total_written` at
    /// the moment of the call) the marker takes effect at — content written
    /// from that offset onward is under `(cols, rows)`, until the next
    /// entry (if any) takes over. [`Self::prune_dim_markers`] drops entries
    /// that can no longer describe the retained window's head; unlike a
    /// bare `Vec`, `pop_front` here is O(1) regardless of how many entries
    /// have accumulated (task0003 D4, review round-2 finding
    /// `0b0c18ff4ab911f4`: the previous `Vec::remove(0)` shifted every
    /// remaining element on each pop, making a single call that needed to
    /// drop many stale entries at once O(n) — quadratic overall for a
    /// long-lived pane that had accumulated many markers before any of them
    /// aged out).
    dim_markers: VecDeque<(u64, u16, u16)>,
}

/// Hard ceiling on `dim_markers`' length, independent of redraw byte volume
/// (task0005 rework D3'', review round-4 finding `6c650908ea8e95e9`,
/// resolving the residual round-3 explicitly deferred as
/// `981230284d7d3273`).
///
/// Measured replay cost (round-4, `crates/term_core/src/terminal_core.rs`'s
/// `replay_segments`, a 0.95 MiB snapshot at the shipping 10 000-line
/// scrollback default): 0 segments → 134 ms / 5 → 176 ms / 20 → 272 ms /
/// 30 → 2078 ms / 50 → 3322 ms / 80 → 5350 ms — a resize-storm-shaped
/// snapshot (a window-edge drag, which emits a `Resize` message per
/// grid-size change with no debounce) can accumulate dozens of entries here
/// with no per-step coalescing available (see [`Self::write_resize_marker`]'s
/// doc for why round-4 reverted the byte-threshold coalescing that used to
/// bound this — it silently misattributed content). Replay cost jumps
/// sharply once segment count crosses roughly 20-30, so this ceiling stays
/// comfortably below that: `crates/term_core/src/bench.rs`'s
/// `segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`
/// empirically confirms replay at exactly this count stays well under 1
/// second and does not exhibit the same superlinear growth within the
/// bounded range.
///
/// Enforced by [`Self::enforce_dim_marker_cap`], called after every
/// [`Self::write_resize_marker`] append: when exceeded, the OLDEST entry is
/// merged into its successor (the successor's offset is pulled back to
/// cover the oldest entry's span, and the oldest entry is dropped) —
/// unlike the reverted byte-threshold coalescing, this only ever touches
/// the SINGLE oldest span each time the cap is exceeded, so every entry
/// more recent than that keeps its EXACT recorded attribution; only the
/// oldest span's content is reattributed (to whichever dims its successor
/// already used), never a more recent one. This does not reintroduce
/// review round-3 finding `ab54fae335086db3` (misattributing RECENT
/// content by retroactively rewriting an already-passed marker's
/// dimensions) — precision is lost only in the single oldest surviving
/// span, which is exactly the trade-off `D3''`'s suggestion (b) accepts.
pub const MAX_DIM_MARKERS: usize = 16;

impl ScrollbackRingBuffer {
    /// Create a new ring buffer with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            capacity,
            write_pos: 0,
            len: 0,
            total_written: 0,
            dim_markers: VecDeque::new(),
        }
    }

    /// Write data to the ring buffer. Overwrites oldest data if full.
    pub fn write(&mut self, data: &[u8]) {
        self.total_written += data.len() as u64;

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

    /// Record a resize marker taking effect at the CURRENT content-stream
    /// offset (task0003 D1): purely a `dim_markers` entry — no bytes are
    /// written to `buf`. `MuxPane::new` / `MuxPane::resize` call this
    /// instead of a plain `write` for every marker they record.
    ///
    /// Coalescing: if the most recent entry is at the EXACT SAME offset (no
    /// real content bytes were written since it was recorded), its
    /// dimensions are UPDATED in place rather than appending a new entry —
    /// this is always safe because nothing was ever attributed to the
    /// superseded dimensions in the first place (zero bytes exist at that
    /// offset for them to describe). Any other case appends a NEW entry.
    ///
    /// task0004 round-4 rework (D2', review round-3 finding
    /// `ab54fae335086db3`, critical): a round-2 fix widened this to coalesce
    /// across up to 8 KiB of INTERVENING real content (treating a small
    /// TUI redraw frame as "negligible"), reusing the earlier entry's
    /// OFFSET while overwriting its DIMENSIONS. That silently reattributed
    /// already-recorded content: bytes written between the two
    /// `write_resize_marker` calls were produced under the FIRST (old)
    /// dimensions, but ended up replayed under the SECOND (new) ones once
    /// the entry describing the first dimensions was overwritten — the
    /// exact resize-interleaved coordinate drift this whole marker
    /// mechanism exists to prevent, reintroduced by the very fix meant to
    /// bound `dim_markers` growth (round-3 findings `ab54fae335086db3` and
    /// `9d4f7176af1b264e`, the latter specifically about
    /// [`Self::attribute_write`]'s correction entries being swallowed the
    /// same way). Reverted to the only sound rule: coalesce ONLY when
    /// literally nothing was recorded in between (offset unchanged).
    ///
    /// A resize storm whose every step emits real (non-zero) redraw bytes
    /// therefore does grow `dim_markers` by one entry per step through THIS
    /// coalescing rule alone — but [`Self::enforce_dim_marker_cap`] (called
    /// right below, task0005 rework D3'') now bounds the total independent
    /// of redraw byte volume, closing the round-3 residual
    /// (`981230284d7d3273`) a byte-count threshold could not close without
    /// reintroducing the misattribution above: the cap merges only the
    /// SINGLE oldest span on overflow, never rewriting a more recent
    /// entry's already-recorded dimensions.
    pub fn write_resize_marker(&mut self, cols: u16, rows: u16) {
        let offset = self.total_written;
        if let Some(last) = self.dim_markers.back_mut() {
            if offset == last.0 {
                last.1 = cols;
                last.2 = rows;
                return;
            }
        }
        self.dim_markers.push_back((offset, cols, rows));
        self.prune_dim_markers();
        self.enforce_dim_marker_cap();
    }

    /// Bound `dim_markers` to at most [`MAX_DIM_MARKERS`] entries (task0005
    /// rework D3''), independent of how many bytes separate each recorded
    /// resize. When the cap is exceeded, the OLDEST entry is merged into
    /// its successor: the successor's `offset` is pulled back to the
    /// dropped entry's offset (extending its span to also cover what the
    /// dropped entry used to describe), and the dropped entry's own
    /// dimensions are discarded. See [`MAX_DIM_MARKERS`]'s doc for why this
    /// only ever loses precision in the single oldest surviving span.
    fn enforce_dim_marker_cap(&mut self) {
        while self.dim_markers.len() > MAX_DIM_MARKERS {
            let Some(dropped) = self.dim_markers.pop_front() else {
                break;
            };
            if let Some(new_oldest) = self.dim_markers.front_mut() {
                new_oldest.0 = dropped.0;
            }
        }
    }

    /// Drop `dim_markers` entries that can no longer be "the dimensions in
    /// effect at the oldest retained byte" for ANY future call (a later
    /// entry already starts at or before the current retained window) —
    /// pure memory hygiene bounding `dim_markers`'s growth to roughly "one
    /// entry per resize since the ring last needed to forget one", not a
    /// correctness requirement: [`Self::read_all`]'s lookup is correct
    /// regardless of whether this has run. Always leaves at least one entry
    /// (the most recent marker still valid as of THIS call) so a later,
    /// larger jump in the retained window is never left with nothing to
    /// fall back on. `pop_front` is O(1) (see the `dim_markers` field doc
    /// for why that matters — task0003 D4, finding `0b0c18ff4ab911f4`).
    fn prune_dim_markers(&mut self) {
        let oldest_offset = self.total_written.saturating_sub(self.capacity as u64);
        while self.dim_markers.len() >= 2 && self.dim_markers[1].0 <= oldest_offset {
            self.dim_markers.pop_front();
        }
    }

    /// Read all accumulated data in chronological order, oldest to newest.
    ///
    /// task0004 round-4 rework (D1', review round-3 findings
    /// `ef69658e6e4d0b05` / `c84c751810cbd8cb`): this is now ALWAYS the
    /// plain content bytes `write` was called with — no marker bytes are
    /// ever synthesized into it. Rounds 1-3 had this method inject fresh
    /// `OSC 777;emterm;resize;…` marker bytes at every `dim_markers` offset
    /// so the wire payload carried dimensions in-band; that meant EVERY
    /// consumer of `read_all()` received those synthetic bytes, including
    /// `mux-read` (`handle_read_pane`), which has no business seeing them
    /// and could have a tail-cut land mid-marker and leak the literal
    /// protocol text into an agent's read result. Consumers that need
    /// structural dimension info now call [`Self::read_segments`] instead;
    /// `read_all` goes back to being a plain byte accessor, with no second
    /// full-buffer copy to insert markers into.
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

    /// Structural counterpart to [`Self::read_all`] (task0004 round-4
    /// rework, D1'): returns the SAME raw content bytes [`Self::read_all`]
    /// would, paired with the `dim_markers` entries still relevant to the
    /// retained window, each expressed as `(position, cols, rows)` where
    /// `position` is a byte offset INTO the returned `Vec<u8>` (not the
    /// ring's absolute stream offset).
    ///
    /// - The entry describing the dimensions in effect at the retained
    ///   window's first byte (the latest entry at or before that offset, if
    ///   any) is reported at `position == 0`, so the invariant "the
    ///   returned segments cover the WHOLE retained window, with no
    ///   dimension-less prefix" holds regardless of ring wraparound.
    /// - Every entry whose offset falls WITHIN the retained window is
    ///   reported at its correct relative position, in ascending order.
    ///
    /// This is the sole surviving descendant of the old `read_all`'s marker
    /// synthesis: instead of encoding `dim_markers` as bytes inside the
    /// payload, callers that need to route a snapshot over the wire carry
    /// this Vec alongside it (`mux_ipc::protocol::DimSegment` /
    /// `encode_snapshot_payload`) and the replay side consumes it as a
    /// structural parameter
    /// (`term_core::terminal_core::TerminalCore::reset_and_replay_segments`),
    /// never by scanning the bytes.
    ///
    /// Returns an empty `Vec` when this ring never called
    /// [`Self::write_resize_marker`] / [`Self::attribute_write`] (e.g. a
    /// bare `ScrollbackRingBuffer` used directly, outside `MuxPane`) — the
    /// caller's replay then degrades to single-dimension (task0004 AC-11).
    pub fn read_segments(&self) -> (Vec<u8>, Vec<(usize, u16, u16)>) {
        let raw = self.read_all();
        if self.dim_markers.is_empty() {
            return (raw, Vec::new());
        }
        let oldest_offset = self.total_written.saturating_sub(self.capacity as u64);

        let mut head: Option<(u16, u16)> = None;
        let mut mid: Vec<(usize, u16, u16)> = Vec::new();
        for &(offset, cols, rows) in &self.dim_markers {
            if offset <= oldest_offset {
                // Ascending order means a LATER entry that still qualifies
                // as "at or before the retained window's head" overwrites
                // an earlier one — the most recent surviving marker wins.
                head = Some((cols, rows));
            } else {
                let pos = ((offset - oldest_offset) as usize).min(raw.len());
                mid.push((pos, cols, rows));
            }
        }

        let mut segments = Vec::with_capacity(mid.len() + 1);
        if let Some((cols, rows)) = head {
            segments.push((0usize, cols, rows));
        }
        segments.extend(mid);
        (raw, segments)
    }

    /// The dimensions the MOST RECENTLY recorded [`Self::write_resize_marker`]
    /// call established, if any (task0003 D5 — see [`Self::attribute_write`]).
    fn last_effective_dims(&self) -> Option<(u16, u16)> {
        self.dim_markers.back().map(|&(_, cols, rows)| (cols, rows))
    }

    /// Append `data`, first recording a resize marker for `(cols, rows)` if
    /// they differ from the dimensions this ring most recently recorded —
    /// task0003 D5 (review round-2 finding `0bebe3e6f7b416dd`): the pane
    /// reader thread calls this (instead of a plain [`Self::write`]) with
    /// the dimensions observed at the moment its `read()` call returned,
    /// attributing the content to the dimensions it was actually PRODUCED
    /// under rather than trusting write-time lock ordering alone. In the
    /// overwhelmingly common case (no concurrent resize) `cols`/`rows`
    /// already match the ring's last-recorded dimensions and this is a
    /// single comparison plus a plain write; the corrective marker only
    /// fires when a resize's own marker won the scrollback lock race ahead
    /// of a reader chunk that was actually produced under the OLD
    /// dimensions.
    pub fn attribute_write(&mut self, cols: u16, rows: u16, data: &[u8]) {
        if self.last_effective_dims() != Some((cols, rows)) {
            self.write_resize_marker(cols, rows);
        }
        self.write(data);
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.write_pos = 0;
        self.len = 0;
        self.total_written = 0;
        self.dim_markers.clear();
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

    /// Number of `dim_markers` entries currently retained. Test-only
    /// observer for the AC-7 bounded-growth regression test.
    #[cfg(test)]
    fn dim_markers_len(&self) -> usize {
        self.dim_markers.len()
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

    // ── task0004 round-4 rework (D1'): dim_markers is the SOLE authority,
    // exposed structurally via read_segments() — read_all() never contains
    // any marker-shaped bytes, at all, regardless of write_resize_marker /
    // attribute_write calls ──────────────────────────────────────────────

    /// AC-9 (review round-3 finding `ef69658e6e4d0b05`): `read_all()` is
    /// completely unaffected by any number of `write_resize_marker` /
    /// `attribute_write` calls — it returns EXACTLY the bytes passed to
    /// `write` / `attribute_write`'s `data` parameter, in order, with zero
    /// synthesized bytes of any kind. This is what makes `mux-read`
    /// (`handle_read_pane`, which calls `read_all()` directly) immune to
    /// ever seeing a protocol remnant: there is nothing to leak because
    /// nothing is ever inserted.
    #[test]
    fn read_all_never_contains_any_synthesized_bytes_regardless_of_resize_calls() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(80, 24);
        rb.write(b"before");
        rb.write_resize_marker(65535, 65535); // extreme dims — would be highly visible if leaked
        rb.attribute_write(120, 40, b"middle");
        rb.write_resize_marker(1, 1);
        rb.write(b"after");
        assert_eq!(rb.read_all(), b"beforemiddleafter");
    }

    /// A ring that never calls `write_resize_marker` behaves exactly as a
    /// plain byte ring, even after wraparound.
    #[test]
    fn read_all_without_any_resize_marker_is_unaffected_by_wraparound() {
        let mut rb = ScrollbackRingBuffer::new(8);
        rb.write(b"ABCDEF");
        rb.write(b"GHI"); // wraps
        assert_eq!(rb.read_all(), b"BCDEFGHI");
    }

    /// `read_segments()` reports the raw bytes (identical to `read_all()`)
    /// paired with the segment for the ring's only recorded resize, at
    /// position 0 (the whole retained window is under those dims).
    #[test]
    fn read_segments_matches_read_all_bytes_and_reports_head_segment() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write_resize_marker(80, 24);
        rb.write(b"hello");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, rb.read_all());
        assert_eq!(bytes, b"hello");
        assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
    }

    /// A ring that never calls `write_resize_marker` reports NO segments —
    /// the caller's replay degrades to single-dimension (task0004 AC-11).
    #[test]
    fn read_segments_reports_no_segments_when_ring_never_recorded_a_resize() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write(b"plain content, no resize ever recorded");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, rb.read_all());
        assert!(segments.is_empty());
    }

    /// Driving the ring past wraparound so the INITIAL marker's offset falls
    /// before the retained window still yields a `read_segments()` head
    /// segment (position 0) describing the dimensions in effect for
    /// whatever content survived — the last marker recorded before the
    /// retained window began.
    #[test]
    fn read_segments_reconstructs_head_segment_after_window_advances_past_it() {
        let capacity = 32;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(80, 24);
        // Push enough plain bytes to advance the retained window well past
        // this marker's offset (0).
        rb.write(&vec![b'x'; capacity * 3]);
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, vec![b'x'; capacity]);
        assert_eq!(
            segments,
            vec![(0usize, 80u16, 24u16)],
            "retained window must be preceded by a segment for the dims that \
             produced it, even though its offset now precedes the window"
        );
    }

    /// Multi-resize variant: after TWO resizes and enough content to advance
    /// the retained window past both offsets, the reconstructed head
    /// segment reflects the SECOND (most recent surviving) resize's
    /// dimensions, not the first.
    #[test]
    fn read_segments_reconstructs_head_segment_using_latest_surviving_resize() {
        let capacity = 64;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(80, 24);
        rb.write(b"some content produced at the first size");
        rb.write_resize_marker(120, 40);
        // Push enough plain bytes to advance the window past both offsets.
        rb.write(&vec![b'y'; capacity * 3]);
        let (bytes, segments) = rb.read_segments();
        assert_eq!(segments, vec![(0usize, 120u16, 40u16)]);
        assert!(
            !bytes
                .windows(b"first size".len())
                .any(|w| w == b"first size"),
            "no trace of the evicted first-resize content should remain: {bytes:?}"
        );
    }

    /// A resize whose offset falls WITHIN the retained window (not just at
    /// its head) is reported at the correct relative position, with the
    /// surrounding plain content bytes unaffected.
    #[test]
    fn read_segments_reports_a_mid_window_segment_at_the_correct_position() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write(b"before");
        rb.write_resize_marker(100, 40);
        rb.write(b"after");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"beforeafter");
        assert_eq!(segments, vec![("before".len(), 100u16, 40u16)]);
    }

    /// `clear()` resets the resize bookkeeping too — a subsequent
    /// `write_resize_marker` after `clear()` behaves like a fresh ring (no
    /// stale offsets from before the clear).
    #[test]
    fn clear_resets_resize_marker_bookkeeping() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write_resize_marker(80, 24);
        rb.write(b"old content");
        rb.clear();
        rb.write_resize_marker(100, 30);
        rb.write(b"new content");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"new content");
        assert_eq!(segments, vec![(0usize, 100u16, 30u16)]);
    }

    // ── task0003 AC-8 (D5, review round-2 finding `0bebe3e6f7b416dd`):
    // attribute_write corrects a marker that won the scrollback lock race
    // ahead of content produced under different (older) dims ─────────────

    /// Simulates the race the finding describes: `MuxPane::resize` wins the
    /// scrollback lock first and records its marker for the NEW dims, but
    /// the reader thread's chunk — which was actually read (and started
    /// processing) BEFORE the resize, so was produced under the OLD dims —
    /// only reaches the ring afterward. `attribute_write` must insert a
    /// corrective entry for the dims it is TOLD the content was produced
    /// under, so that content is not misattributed to the resize's (newer,
    /// wrong) dims at replay.
    #[test]
    fn attribute_write_corrects_a_marker_that_won_the_lock_race_ahead_of_stale_content() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(100, 40);
        rb.attribute_write(80, 24, b"stale-dims content");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"stale-dims content");
        assert_eq!(
            segments,
            vec![(0usize, 80u16, 24u16)],
            "content attributed via attribute_write must be preceded by a \
             segment for the dims it was actually produced under, correcting \
             a marker that won the lock race for different dims"
        );
    }

    /// AC-3 (task0004 round-4 rework, review round-3 finding
    /// `9d4f7176af1b264e`): a correction entry for bytes read BEFORE a
    /// resize survives SUBSEQUENT recording at the resize's own dims — the
    /// three-step sequence the finding names: resize, stale chunk (via
    /// `attribute_write` at the OLD dims), then a fresh chunk recorded at
    /// the NEW dims. The stale chunk must keep its OLD-dims attribution
    /// even after the fresh chunk is recorded.
    ///
    /// Confirmed to fail pre-fix: against the removed
    /// `RESIZE_MARKER_COALESCE_MAX_BYTES`-based coalescing (which merged
    /// any two `write_resize_marker` calls within 8 KiB of each other by
    /// overwriting the EARLIER entry's dimensions in place), the fresh
    /// chunk's `write_resize_marker(100, 40)` call — happening right after
    /// the short `stale-dims content` write — landed within that window and
    /// overwrote the entry this test expects to survive at `(80, 24)`,
    /// collapsing both entries into one at `(100, 40)` and misattributing
    /// the stale chunk to the new dims.
    #[test]
    fn attribute_write_correction_survives_a_subsequent_recording_at_new_dims() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        // Step 1: resize records the NEW dims marker.
        rb.write_resize_marker(100, 40);
        // Step 2: a stale chunk (produced before the resize) is attributed
        // to the OLD dims, correcting the marker that won the lock race.
        rb.attribute_write(80, 24, b"stale chunk");
        // Step 3: a fresh chunk, genuinely produced at the NEW dims,
        // arrives next.
        rb.attribute_write(100, 40, b"fresh chunk");

        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"stale chunkfresh chunk");
        assert_eq!(
            segments,
            vec![(0usize, 80u16, 24u16), ("stale chunk".len(), 100u16, 40u16),],
            "the stale chunk's correction must survive the subsequent \
             fresh-chunk recording, not get swallowed back into the resize's \
             dims"
        );
    }

    /// The overwhelmingly common case (no concurrent resize): dims already
    /// match the ring's last-recorded dims, so `attribute_write` is just a
    /// plain write — no extra marker.
    #[test]
    fn attribute_write_is_a_plain_write_when_dims_already_match() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(80, 24);
        rb.attribute_write(80, 24, b"normal content");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"normal content");
        assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
        assert_eq!(
            rb.dim_markers_len(),
            1,
            "no extra marker when the attributed dims already match"
        );
    }

    /// `attribute_write` on a ring that never recorded ANY marker (e.g. a
    /// bare `ScrollbackRingBuffer` used directly, outside `MuxPane`) still
    /// seeds one for the FIRST call's dims — `last_effective_dims()` is
    /// `None` initially, which never equals `Some((cols, rows))`.
    #[test]
    fn attribute_write_seeds_a_marker_on_a_ring_with_no_prior_marker() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.attribute_write(80, 24, b"first content");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"first content");
        assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
    }

    // ── task0004 round-4 rework (D2', review round-3 finding
    // `ab54fae335086db3`): coalescing reverted to the only sound rule ─────

    /// Two `write_resize_marker` calls with literally ZERO bytes recorded
    /// between them (the safe case: nothing exists yet to misattribute)
    /// coalesce into a single entry, taking the LATEST dimensions.
    #[test]
    fn write_resize_marker_coalesces_only_at_the_exact_same_offset() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(80, 24);
        rb.write_resize_marker(81, 25); // zero bytes since the previous call
        rb.write_resize_marker(82, 26); // zero bytes since the previous call
        assert_eq!(
            rb.dim_markers_len(),
            1,
            "consecutive resize calls with zero intervening bytes must \
             coalesce (nothing was ever recorded under the superseded dims)"
        );
        rb.write(b"content");
        let (_, segments) = rb.read_segments();
        assert_eq!(segments, vec![(0usize, 82u16, 26u16)]);
    }

    /// ANY real content between two resize calls — even a single byte —
    /// must NOT be coalesced away: doing so would misattribute that content
    /// to the wrong dimensions (review round-3 finding `ab54fae335086db3`,
    /// critical — the round-2 8 KiB-threshold coalescing did exactly this
    /// for "negligible" redraws).
    ///
    /// Confirmed to fail pre-fix: against the removed
    /// `RESIZE_MARKER_COALESCE_MAX_BYTES` (8 KiB) rule, this single byte of
    /// intervening content was well under the threshold, so the second
    /// `write_resize_marker` call coalesced into the first — overwriting
    /// its dims and misattributing the 1-byte write to (81, 25) instead of
    /// the (80, 24) it was actually recorded under.
    #[test]
    fn write_resize_marker_never_coalesces_across_any_real_content() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(80, 24);
        rb.write(b"X"); // a single byte of real content
        rb.write_resize_marker(81, 25);
        assert_eq!(
            rb.dim_markers_len(),
            2,
            "even one byte of real content between two resize calls must \
             prevent coalescing"
        );
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"X");
        assert_eq!(
            segments,
            vec![(0usize, 80u16, 24u16), (1usize, 81u16, 25u16)],
            "the single byte must stay attributed to the FIRST dims, not the \
             second"
        );
    }

    /// AC-2 end-to-end: a resize immediately followed by under-threshold
    /// (well under the OLD 8 KiB coalescing window) real output, then
    /// another resize, must keep the intervening output under its ORIGINAL
    /// dimensions — never retroactively reattributed by a later resize.
    #[test]
    fn resize_then_small_output_then_resize_keeps_original_attribution() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(80, 24);
        rb.write(b"small output under 8KiB");
        rb.write_resize_marker(120, 40);
        rb.write(b"more output");
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes, b"small output under 8KiBmore output");
        assert_eq!(
            segments,
            vec![
                (0usize, 80u16, 24u16),
                ("small output under 8KiB".len(), 120u16, 40u16),
            ]
        );
    }

    // ── task0005 rework D3'' (review round-4 finding `6c650908ea8e95e9`,
    // resolving the round-3 residual `981230284d7d3273`): a resize storm
    // whose EVERY step emits real (non-empty) redraw bytes — so none of the
    // steps can coalesce via the exact-offset rule above — must still
    // produce a BOUNDED `dim_markers` count, independent of redraw byte
    // volume, via `enforce_dim_marker_cap` (not a byte-threshold coalescing
    // rule, which round-4 already proved unsound above). ──────────────────

    /// AC-4: a resize storm where each step's redraw exceeds the old 8 KiB
    /// coalescing threshold — so exact-offset coalescing never fires — is
    /// still bounded to `MAX_DIM_MARKERS` entries by the daemon-side cap,
    /// regardless of how many steps occur.
    ///
    /// Confirmed to fail pre-fix: before `enforce_dim_marker_cap` existed,
    /// this recorded exactly `step_count` (50) entries — the prior version
    /// of this test asserted that UNBOUNDED growth as the then-accepted
    /// trade-off (round-3 finding `981230284d7d3273`); task0005 changes
    /// that trade-off, so this test now asserts the opposite.
    #[test]
    fn resize_storm_with_large_per_step_redraw_stays_bounded_by_max_dim_markers() {
        let mut rb = ScrollbackRingBuffer::new(4 * 1024 * 1024);
        let step_count = 50u16;
        let redraw = vec![b'r'; 10 * 1024]; // > the old 8 KiB coalescing threshold
        for step in 0..step_count {
            rb.write_resize_marker(80 + step, 24);
            rb.write(&redraw);
        }
        assert_eq!(
            rb.dim_markers_len(),
            MAX_DIM_MARKERS,
            "even though none of the {step_count} resize calls could \
             coalesce (each redraw exceeds the old 8 KiB coalescing \
             threshold), the daemon-side cap must still bound the recorded \
             segment count to MAX_DIM_MARKERS"
        );
    }

    /// D3'' merge semantics: when the cap is exceeded, ONLY the single
    /// oldest span is folded into its successor — every entry more recent
    /// than that keeps its EXACT recorded dimensions. This is what
    /// distinguishes the cap from the unsound byte-threshold coalescing
    /// round-4 reverted (review round-3 finding `ab54fae335086db3`): that
    /// fix retroactively rewrote an ALREADY-RECORDED entry's dimensions,
    /// misattributing recent content; this cap only ever discards the
    /// entry about to fall off the front, extending its SUCCESSOR's span
    /// backward without touching the successor's own dimensions.
    #[test]
    fn enforce_dim_marker_cap_merges_only_the_oldest_span_preserving_recent_attribution() {
        let mut rb = ScrollbackRingBuffer::new(4 * 1024 * 1024);
        let extra = 3usize;
        let total_steps = MAX_DIM_MARKERS + extra;
        let content_per_step = b"distinct-step-content;";
        for step in 0..total_steps {
            // `rows` encodes the step index so each entry is
            // distinguishable in the assertion below.
            rb.write_resize_marker(80, 24 + step as u16);
            rb.write(content_per_step);
        }
        assert_eq!(
            rb.dim_markers_len(),
            MAX_DIM_MARKERS,
            "cap must hold even with {extra} entries beyond it"
        );
        let (_, segments) = rb.read_segments();
        assert_eq!(segments.len(), MAX_DIM_MARKERS);
        // The surviving entries' rows values are the MOST RECENT
        // `MAX_DIM_MARKERS` steps — steps 0..extra were merged away (their
        // OWN dimensions discarded, folded into step `extra`'s span), but
        // every entry from `extra` onward survives with its EXACT
        // originally-recorded dimensions.
        let expected_rows: Vec<u16> = (extra..total_steps).map(|s| 24 + s as u16).collect();
        let actual_rows: Vec<u16> = segments.iter().map(|&(_, _, rows)| rows).collect();
        assert_eq!(
            actual_rows, expected_rows,
            "every surviving entry must keep its EXACT originally-recorded \
             dimensions — only the discarded oldest entries lose precision"
        );
    }

    // ── task0003 AC-7 (D4, review round-2 finding `0b0c18ff4ab911f4`):
    // pruning is linear (VecDeque::pop_front), not quadratic ────────────

    /// D3'' interaction: `enforce_dim_marker_cap` (count-based) and
    /// `prune_dim_markers` (retained-window-based) cooperate correctly — a
    /// resize storm whose total marker count exceeds `MAX_DIM_MARKERS` AND
    /// whose accumulated content exceeds the ring's capacity (so BOTH
    /// eviction mechanisms fire on the same run) never leaves `dim_markers`
    /// above the cap, and `read_segments` still reconstructs a coherent
    /// head segment for whatever content survives.
    ///
    /// Supersedes `prune_dim_markers_collapses_a_large_backlog_to_a_bounded_tail_in_one_pass`
    /// (task0003 AC-7 / review round-2 finding `0b0c18ff4ab911f4`): that
    /// test's premise — building a 2,000-entry `dim_markers` backlog before
    /// a single mass-eviction write — can no longer be constructed now that
    /// `enforce_dim_marker_cap` keeps the count at `MAX_DIM_MARKERS` (16)
    /// continuously; a backlog that large would need exactly the unbounded
    /// growth task0005 closes. The O(1)-vs-O(n) `pop_front` concern that
    /// test guarded is consequently moot at this bound (16 entries is cheap
    /// either way); this replacement instead pins the CORRECTNESS of the
    /// two eviction mechanisms cooperating, which is the property that
    /// actually matters now.
    #[test]
    fn dim_markers_stays_capped_when_window_pruning_and_count_cap_both_fire() {
        let chunk_len = 4096;
        let marker_count = 40u32; // well above MAX_DIM_MARKERS
        // Small capacity relative to the total content written, so the
        // retained-window pruning ALSO advances past many early markers —
        // both eviction mechanisms are exercised on the same run.
        let capacity = (marker_count as usize / 2) * chunk_len;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        for i in 0..marker_count {
            rb.write_resize_marker(80, 24 + (i % 50) as u16);
            rb.write(&vec![b'q'; chunk_len]);
            assert!(
                rb.dim_markers_len() <= MAX_DIM_MARKERS,
                "dim_markers must never exceed MAX_DIM_MARKERS, even \
                 mid-storm (saw {} after marker {})",
                rb.dim_markers_len(),
                i
            );
        }
        let (bytes, segments) = rb.read_segments();
        assert_eq!(bytes.len(), capacity.min(marker_count as usize * chunk_len));
        assert!(
            !segments.is_empty(),
            "a ring that recorded resize markers must still report a head \
             segment for the retained window"
        );
        assert_eq!(
            segments[0].0, 0,
            "the head segment must always describe position 0 of the \
             retained window"
        );
    }

    /// Historical form of the test above, kept to pin the ORIGINAL
    /// task0003 AC-7 guarantee in isolation: `prune_dim_markers` alone
    /// (the count cap not yet in play, capacity comfortably larger than the
    /// content written) still collapses a large backlog to a bounded tail
    /// in one pass once the retained window is deliberately advanced past
    /// it — the property this test previously required a 2,000-entry
    /// backlog to observe now shows up at the (much smaller) count-cap
    /// bound instead, so this variant caps the backlog at `MAX_DIM_MARKERS`
    /// (the largest `dim_markers` can ever actually reach) rather than
    /// asserting an unreachable precondition.
    #[test]
    fn prune_dim_markers_collapses_a_capped_backlog_to_a_bounded_tail_in_one_pass() {
        let chunk_len = 8 * 1024 + 1;
        let marker_count = MAX_DIM_MARKERS as u32;
        // Capacity comfortably larger than the whole backlog so every
        // marker's offset is still within the retained window before the
        // deliberate mass-eviction write below.
        let capacity = marker_count as usize * chunk_len + (1024 * 1024);
        let mut rb = ScrollbackRingBuffer::new(capacity);
        for i in 0..marker_count {
            rb.write_resize_marker(80, 24 + (i % 50) as u16);
            rb.write(&vec![b'q'; chunk_len]);
        }
        assert_eq!(
            rb.dim_markers_len(),
            marker_count as usize,
            "test prerequisite: all markers must still be within the \
             retained window (none pruned yet)"
        );
        // Advance the retained window past every recorded offset at once
        // (a single write >= capacity hits the ring's "keep only the tail"
        // fast path, so this itself stays O(capacity), not O(capacity^2)).
        rb.write(&vec![b'r'; capacity + 1]);
        rb.write_resize_marker(100, 40);
        assert!(
            rb.dim_markers_len() <= 2,
            "a single prune pass must collapse a {marker_count}-entry backlog \
             down to a bounded tail, got {}",
            rb.dim_markers_len()
        );
    }
}
