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

/// Build the in-band resize marker recorded into a pane's scrollback stream
/// at each PTY resize (IMPLEMENTATION.md D1/D2, task0001): a private OSC 777
/// `resize` extension carrying the pane's new dimensions —
/// `ESC ] 777 ; emterm ; resize ; <cols> ; <rows> BEL`.
///
/// This is the fix for the resize-interleaved scrollback replay coordinate
/// drift (`tmp/apt-progress-bar-regression-2026-07-09.md` PROBE D): bytes
/// recorded for different terminal row counts coexist serially in a pane's
/// scrollback, and a replay that feeds them all into a core fixed at one row
/// count misinterprets DECSTBM / CUP coordinates recorded for the OTHER row
/// count, mixing content from two logical output lines onto one row. The
/// marker lets a replay consumer resize its core to match the dimensions the
/// FOLLOWING bytes were produced for (see `term_core::terminal_core`'s
/// `find_resize_marker` / `TerminalCore::reset_and_replay`).
///
/// task0003 D2 (review round-2 finding `602e685494248cbb`): this used to be
/// an independent literal, hand-mirrored by a SEPARATE literal in
/// `term_core::terminal_core`'s decoder, with each side enforcing its own
/// dimension bounds — the encoder accepted any `u16` while the decoder
/// rejected anything past a private cap, so a legitimate large resize could
/// silently lose its marker at replay. `term_core` has no dependency on
/// this (`emterm` mux) crate, but this crate already depends on `term_core`,
/// so the byte format and its accepted range are now owned there
/// (`term_core::terminal_core::resize_marker_bytes` /
/// `RESIZE_MARKER_MAX_COLS` / `RESIZE_MARKER_MAX_ROWS`) and this function is
/// a thin re-export — the two sides can no longer drift.
pub fn resize_marker_bytes(cols: u16, rows: u16) -> Vec<u8> {
    term_core::terminal_core::resize_marker_bytes(cols, rows)
}

/// Maximum bytes of REAL (non-marker) content that may separate two
/// `write_resize_marker` calls for them to still be coalesced into a single
/// `dim_markers` entry (task0003 D4, review round-2 finding
/// `5d1a4e9509365517`).
///
/// The existing "coalesce adjacent markers" behavior only collapsed markers
/// with LITERALLY ZERO bytes between them, but a real drag-resize has the
/// TUI redraw its frame at each intermediate size, so consecutive markers
/// almost always have SOME output between them and the zero-byte rule never
/// fires. Left unbounded, a single drag produces one `dim_markers` entry per
/// intermediate size, and every later snapshot rebuild / replay pays one
/// full-scrollback reflow per surviving entry (until they age out of the
/// retained window). Treating "a single TUI frame's worth of redraw" as
/// negligible bounds this: a dimension change superseding a previous one
/// with at most this many bytes of output since is folded into the SAME
/// entry (the position is kept, only the dimensions are updated) rather
/// than appended as a new one. Chosen generously above a full-screen redraw
/// for a large terminal (heavily SGR-styled output can run several bytes
/// per cell) while still being far below "a real, distinguishable command's
/// output" — a `cat` of a multi-KB file right after a resize is NOT
/// coalesced away.
const RESIZE_MARKER_COALESCE_MAX_BYTES: u64 = 8 * 1024;

/// Circular byte buffer with fixed capacity.
///
/// `dim_markers` is the SOLE authority for "which dimensions were in effect
/// at a given byte offset" (task0003 D1, review round-2 findings
/// `a6ab9b340119beed` / `15c54fb74bb91ec7`): resize-marker bytes are NEVER
/// written into `buf` at all — [`Self::write_resize_marker`] only records
/// `(offset, cols, rows)` here, and [`Self::read_all`] synthesizes fresh
/// marker bytes from these entries at read time. PTY-sourced content can
/// therefore never contribute a marker (nothing forged, nested, or smuggled
/// through any write path can ever be mistaken for one), because there is
/// no marker-shaped content for it to collide with in the first place —
/// `buf` holds ONLY the plain byte stream `write` was called with. This
/// replaces the previous design (round-1 rework) where a marker's bytes
/// were written into `buf` like ordinary content and `read_all` had to
/// special-case reconstructing just the one at the retained window's head
/// after ring-wraparound eviction; that reconstruction is now the general
/// case for every surviving entry, at any offset, not only the head.
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
    /// D4 coalescing (review round-2 finding `5d1a4e9509365517`): if the
    /// most recent entry is still within
    /// [`RESIZE_MARKER_COALESCE_MAX_BYTES`] of the current offset — i.e.
    /// only a negligible amount of real content (a redraw frame, not a
    /// distinguishable command's output) has been written since — its
    /// dimensions are UPDATED in place rather than appending a new entry.
    /// This is what keeps a drag-resize (many intermediate sizes, each
    /// followed by a TUI redraw) from leaving one `dim_markers` entry per
    /// intermediate size, each costing a full-scrollback reflow on every
    /// later replay.
    pub fn write_resize_marker(&mut self, cols: u16, rows: u16) {
        let offset = self.total_written;
        if let Some(last) = self.dim_markers.back_mut() {
            if offset.saturating_sub(last.0) <= RESIZE_MARKER_COALESCE_MAX_BYTES {
                last.1 = cols;
                last.2 = rows;
                return;
            }
        }
        self.dim_markers.push_back((offset, cols, rows));
        self.prune_dim_markers();
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

    /// Read all accumulated data in chronological order.
    /// Returns a Vec containing the buffer contents from oldest to newest.
    ///
    /// When [`Self::write_resize_marker`] has ever been called on this ring,
    /// the returned bytes have fresh marker bytes synthesized from
    /// `dim_markers` (task0003 D1) at every offset still relevant to the
    /// retained window:
    ///
    /// - The entry describing the dimensions in effect at the retained
    ///   window's first byte (the latest entry at or before that offset, if
    ///   any) is prepended, so the invariant "the retained window is
    ///   preceded by a marker describing the dimensions in effect at its
    ///   first byte" holds regardless of ring wraparound.
    /// - Every entry whose offset falls WITHIN the retained window is
    ///   spliced in at its correct relative position, in order.
    ///
    /// A ring that never received a [`Self::write_resize_marker`] call (e.g.
    /// a bare `ScrollbackRingBuffer` used directly, outside `MuxPane`)
    /// behaves exactly as before — no synthetic bytes are ever introduced.
    pub fn read_all(&self) -> Vec<u8> {
        let raw = self.read_all_raw();
        if self.dim_markers.is_empty() {
            return raw;
        }
        let oldest_offset = self.total_written.saturating_sub(self.capacity as u64);

        let mut head_marker: Option<(u16, u16)> = None;
        let mut mid_markers: Vec<(usize, u16, u16)> = Vec::new();
        for &(offset, cols, rows) in &self.dim_markers {
            if offset <= oldest_offset {
                // Ascending order means a LATER entry that still qualifies
                // as "at or before the retained window's head" overwrites
                // an earlier one — the most recent surviving marker wins.
                head_marker = Some((cols, rows));
            } else {
                let pos = ((offset - oldest_offset) as usize).min(raw.len());
                mid_markers.push((pos, cols, rows));
            }
        }

        if head_marker.is_none() && mid_markers.is_empty() {
            return raw;
        }

        let mut out = Vec::with_capacity(raw.len() + (mid_markers.len() + 1) * 32);
        if let Some((cols, rows)) = head_marker {
            out.extend_from_slice(&resize_marker_bytes(cols, rows));
        }
        let mut cursor = 0usize;
        for (pos, cols, rows) in mid_markers {
            out.extend_from_slice(&raw[cursor..pos]);
            out.extend_from_slice(&resize_marker_bytes(cols, rows));
            cursor = pos;
        }
        out.extend_from_slice(&raw[cursor..]);
        out
    }

    /// The raw ring contents (no marker synthesis) — plain content bytes
    /// only, since [`Self::write_resize_marker`] never contributes to `buf`
    /// (task0003 D1).
    fn read_all_raw(&self) -> Vec<u8> {
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

    // ── resize_marker_bytes (task0001 AC-1..AC-4, IMPLEMENTATION.md D2) ──

    #[test]
    fn test_resize_marker_bytes_format() {
        assert_eq!(
            resize_marker_bytes(120, 48),
            b"\x1b]777;emterm;resize;120;48\x07".to_vec()
        );
    }

    #[test]
    fn test_resize_marker_bytes_distinguishes_cols_and_rows() {
        // Guards against a copy-paste swap of the two fields.
        assert_ne!(resize_marker_bytes(80, 24), resize_marker_bytes(24, 80));
    }

    /// review round-1 rework, finding `4005fe364386682d` (medium) / task0002
    /// AC-10: drift-guard round-trip. The encoder here
    /// (`resize_marker_bytes`) and `term_core`'s decoder
    /// (`RESIZE_MARKER_PREFIX` / `parse_resize_marker_dims`, a SEPARATE
    /// crate this one depends on but which cannot depend back) hold
    /// independent literals for the marker byte format — nothing at
    /// compile time catches one changing without the other. This test
    /// feeds the encoder's actual output through a real
    /// `term_core::TerminalCore` via `reset_and_replay` (the same entry
    /// point every replay consumer uses) and proves it is still
    /// RECOGNIZED as a resize instruction — not merely byte-format-pinned
    /// (`test_resize_marker_bytes_format` above already does that) — using
    /// the same scroll-region + cursor-addressed-redraw technique
    /// task0001's coordinate-drift regression tests use: the redraw's
    /// correct placement depends on the row count the marker encodes
    /// actually taking effect. A future drift between the two crates'
    /// literals would make the marker an inert, unrecognized OSC, and this
    /// test would fail with the exact coordinate-mixing symptom the
    /// marker protocol exists to prevent — loud and immediate, not a
    /// silent regression discovered later in an unrelated test.
    ///
    /// task0003 D2 update: the encoder is now a thin re-export of
    /// `term_core::terminal_core::resize_marker_bytes` (the two are the
    /// SAME function), so this test is now more of a permanent pin than a
    /// drift guard — kept as-is since it still proves the marker survives
    /// a full round-trip through a real `TerminalCore`.
    #[test]
    fn resize_marker_bytes_round_trips_through_term_core_decoder_drift_guard() {
        // Mirrors the EXACT shape and magnitudes of the proven task0001
        // regression test `mux::ipc::pty_spawn::
        // resize_marker_fix_tui_cursor_addressed_recording_replays_without_cross_line_mixing`
        // (rows 30 vs 32, replay at 32) — empirically confirmed (during
        // this test's own development) to fail loudly when the decoder is
        // drifted from the encoder, unlike a naively-scaled-down variant: a
        // too-small recording (few fill lines) or a too-large row delta
        // lets the replay core's out-of-viewport CUP clamping coincidentally
        // land BOTH phases' status redraws on the same row even when the
        // marker is never recognized at all, masking the drift instead of
        // revealing it as content mixing.
        let cols: u16 = 100;
        let rows_a: u16 = 32;
        let rows_b: u16 = 30;
        let replay_rows: u16 = 32;

        let mut recording = resize_marker_bytes(cols, rows_a);
        for i in 0..rows_a.max(rows_b) + 20 {
            recording.extend_from_slice(format!("chat history line {i}\r\n").as_bytes());
        }
        recording.extend_from_slice(b"\n\x1b7");
        recording.extend_from_slice(format!("\x1b[0;{}r", rows_a - 1).as_bytes());
        recording.extend_from_slice(b"\x1b8\x1b[1A");
        for tick in 0..3u32 {
            recording.extend_from_slice(format!("chat reply A line {tick}\r\n").as_bytes());
            recording.extend_from_slice(
                format!("\x1b7\x1b[{rows_a};0fSTATUS-A[{tick}]\x1b8").as_bytes(),
            );
        }
        recording.extend_from_slice(&resize_marker_bytes(cols, rows_b));
        recording.extend_from_slice(b"\n\x1b7");
        recording.extend_from_slice(format!("\x1b[0;{}r", rows_b - 1).as_bytes());
        recording.extend_from_slice(b"\x1b8\x1b[1A");
        for tick in 0..3u32 {
            recording.extend_from_slice(format!("chat reply B line {tick}\r\n").as_bytes());
            recording.extend_from_slice(
                format!("\x1b7\x1b[{rows_b};0fSTATUS-B[{tick}]\x1b8").as_bytes(),
            );
        }

        let mut core = term_core::terminal_core::TerminalCore::new(cols, replay_rows, 10_000);
        core.reset_and_replay(&recording);
        let mut tainted = Vec::new();
        for r in 0..replay_rows {
            let line = core.get_line_text(r);
            if line.contains("STATUS-") && line.contains(" line ") {
                tainted.push(format!("row {r}: {line}"));
            }
        }
        assert!(
            tainted.is_empty(),
            "resize_marker_bytes -> term_core decoder round-trip must show zero \
             cross-phase content mixing (a drift between the two crates' literals \
             would reintroduce the coordinate-mixing bug this marker protocol \
             exists to prevent): {tainted:?}"
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

    // ── task0003 D1 (review round-2 findings `a6ab9b340119beed` /
    // `15c54fb74bb91ec7`): dim_markers is the SOLE authority — marker bytes
    // never occupy ring space, so read_all() synthesizes them at read time ──

    /// `write_resize_marker` behaves like `write(&resize_marker_bytes(..))`
    /// content-wise when the ring never wraps: `read_all` returns the exact
    /// same bytes a plain `write` would have produced.
    #[test]
    fn write_resize_marker_matches_plain_write_before_any_wraparound() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write_resize_marker(80, 24);
        rb.write(b"hello");
        assert_eq!(
            rb.read_all(),
            [resize_marker_bytes(80, 24), b"hello".to_vec()].concat()
        );
    }

    /// A ring that never calls `write_resize_marker` is completely
    /// unaffected by the new reconstruction logic — `read_all` returns raw
    /// bytes exactly as before, even after wraparound.
    #[test]
    fn read_all_without_any_resize_marker_is_unaffected_by_wraparound() {
        let mut rb = ScrollbackRingBuffer::new(8);
        rb.write(b"ABCDEF");
        rb.write(b"GHI"); // wraps
        assert_eq!(rb.read_all(), b"BCDEFGHI");
    }

    /// Driving the ring past wraparound so the INITIAL marker's offset falls
    /// before the retained window still yields a `read_all()` that starts
    /// with a marker describing the dimensions in effect for whatever
    /// content survived — the last marker recorded before the retained
    /// window began. Since marker bytes never occupied ring space to begin
    /// with (task0003 D1), there is no "evicted marker bytes" case to
    /// reconstruct from partial remnants any more — this is just the
    /// ordinary "marker offset precedes the retained window" branch of
    /// `read_all`.
    #[test]
    fn read_all_reconstructs_head_marker_after_window_advances_past_it() {
        let capacity = 32;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(80, 24);
        // Push enough plain bytes to advance the retained window well past
        // this marker's offset (0).
        rb.write(&vec![b'x'; capacity * 3]);
        let out = rb.read_all();
        let expected = [resize_marker_bytes(80, 24), vec![b'x'; capacity]].concat();
        assert_eq!(
            out, expected,
            "retained window must be preceded by a marker for the dims that \
             produced it, even though its offset now precedes the window"
        );
    }

    /// Multi-resize variant: after TWO resizes and enough content to advance
    /// the retained window past both offsets, the reconstructed head marker
    /// reflects the SECOND (most recent surviving) resize's dimensions, not
    /// the first.
    #[test]
    fn read_all_reconstructs_head_marker_using_latest_surviving_resize() {
        let capacity = 64;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(80, 24);
        rb.write(b"some content produced at the first size");
        rb.write_resize_marker(120, 40);
        // Push enough plain bytes to advance the window past both offsets.
        rb.write(&vec![b'y'; capacity * 3]);
        let out = rb.read_all();
        assert!(
            out.starts_with(&resize_marker_bytes(120, 40)),
            "head marker must reflect the most recent resize whose offset \
             still precedes the retained window: {out:?}"
        );
        let first_marker = resize_marker_bytes(80, 24);
        assert!(
            !out.windows(first_marker.len()).any(|w| w == first_marker),
            "no trace of the superseded first marker should remain: {out:?}"
        );
        assert!(
            !out.windows(b"first size".len()).any(|w| w == b"first size"),
            "no trace of the evicted first-resize content should remain: {out:?}"
        );
    }

    /// A marker whose offset falls WITHIN the retained window (not just at
    /// its head) is spliced into `read_all`'s output at the correct
    /// position, with the surrounding plain content preserved on both
    /// sides — the general case task0003 D1 extends the reconstruction to
    /// (previously, only the head position was ever reconstructed).
    #[test]
    fn read_all_splices_a_mid_window_marker_at_the_correct_position() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write(b"before");
        rb.write_resize_marker(100, 40);
        rb.write(b"after");
        assert_eq!(
            rb.read_all(),
            [
                b"before".to_vec(),
                resize_marker_bytes(100, 40),
                b"after".to_vec()
            ]
            .concat()
        );
    }

    /// Since marker bytes never physically occupy ring space (task0003 D1),
    /// ring eviction can never cut a marker's byte sequence in half — the
    /// entire "truncated marker remnant surfaces as garbage" failure mode
    /// the previous design needed a special reconstruction branch for is now
    /// structurally impossible. This test drives the SAME kind of tight,
    /// exact-eviction-boundary scenario the old truncation test did (a
    /// resize followed by content sized so the retained window's boundary
    /// would, under the OLD byte-embedded design, have landed mid-marker)
    /// and confirms `read_all()` still produces exactly
    /// `marker_bytes + surviving_content` — clean, with no remnant of any
    /// kind, by construction rather than by a remnant-dropping branch.
    #[test]
    fn read_all_never_produces_a_marker_byte_remnant_even_at_a_tight_eviction_boundary() {
        let marker = resize_marker_bytes(65500, 65500); // fixed-width digits, easy to reason about
        let marker_len = marker.len();
        let capacity = marker_len + 4;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(65500, 65500);
        // Content sized so the OLD design's ring-wraparound eviction would
        // have landed exactly mid-marker; content bytes alone now occupy
        // the ring (the marker itself was never IN it).
        let content_len = marker_len + 4 - 2;
        rb.write(&vec![b'z'; content_len]);

        let out = rb.read_all();
        let expected = [marker, vec![b'z'; content_len]].concat();
        assert_eq!(
            out, expected,
            "a clean marker must precede the full surviving content, with no \
             partial-marker-byte remnant possible"
        );
    }

    /// `clear()` resets the resize-marker bookkeeping too — a subsequent
    /// `write_resize_marker` after `clear()` behaves like a fresh ring (no
    /// stale marker offsets from before the clear).
    #[test]
    fn clear_resets_resize_marker_bookkeeping() {
        let mut rb = ScrollbackRingBuffer::new(1024);
        rb.write_resize_marker(80, 24);
        rb.write(b"old content");
        rb.clear();
        rb.write_resize_marker(100, 30);
        rb.write(b"new content");
        assert_eq!(
            rb.read_all(),
            [resize_marker_bytes(100, 30), b"new content".to_vec()].concat()
        );
    }

    // ── task0003 AC-8 (D5, review round-2 finding `0bebe3e6f7b416dd`):
    // attribute_write corrects a marker that won the scrollback lock race
    // ahead of content produced under different (older) dims ─────────────

    /// Simulates the race the finding describes: `MuxPane::resize` wins the
    /// scrollback lock first and records its marker for the NEW dims, but
    /// the reader thread's chunk — which was actually read (and started
    /// processing) BEFORE the resize, so was produced under the OLD dims —
    /// only reaches the ring afterward. `attribute_write` must insert a
    /// corrective marker for the dims it is TOLD the content was produced
    /// under, so that content is not misattributed to the resize's (newer,
    /// wrong) dims at replay.
    #[test]
    fn attribute_write_corrects_a_marker_that_won_the_lock_race_ahead_of_stale_content() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(100, 40);
        rb.attribute_write(80, 24, b"stale-dims content");
        assert_eq!(
            rb.read_all(),
            [resize_marker_bytes(80, 24), b"stale-dims content".to_vec()].concat(),
            "content attributed via attribute_write must be preceded by a \
             marker for the dims it was actually produced under, correcting \
             a marker that won the lock race for different dims"
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
        assert_eq!(
            rb.read_all(),
            [resize_marker_bytes(80, 24), b"normal content".to_vec()].concat()
        );
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
        assert_eq!(
            rb.read_all(),
            [resize_marker_bytes(80, 24), b"first content".to_vec()].concat()
        );
    }

    // ── task0003 D4 (review round-2 finding `5d1a4e9509365517`): recording-
    // side coalescing bounds dim_markers growth across a drag-resize-shaped
    // sequence (many dimension changes, each followed by redraw bytes) ────

    /// A dimension change following the previous one with only a SMALL
    /// (below-threshold) amount of content in between replaces the existing
    /// entry rather than appending — simulating a drag-resize's "redraw a
    /// frame, then immediately resize again" pattern.
    #[test]
    fn write_resize_marker_coalesces_when_negligible_content_intervenes() {
        let mut rb = ScrollbackRingBuffer::new(4096);
        rb.write_resize_marker(80, 24);
        rb.write(b"tiny redraw"); // well under the coalesce threshold
        rb.write_resize_marker(81, 25);
        rb.write(b"tiny redraw 2");
        rb.write_resize_marker(82, 26);
        assert_eq!(
            rb.dim_markers_len(),
            1,
            "consecutive markers separated only by negligible content must \
             coalesce into a single dim_markers entry"
        );
        // The coalesced entry's position is retained from the FIRST of the
        // coalesced markers, but its dimensions are the LATEST — read_all
        // must reflect the final dimensions.
        assert_eq!(
            rb.read_all(),
            [
                resize_marker_bytes(82, 26),
                b"tiny redraw".to_vec(),
                b"tiny redraw 2".to_vec(),
            ]
            .concat()
        );
    }

    /// A dimension change following the previous one with MORE than
    /// [`RESIZE_MARKER_COALESCE_MAX_BYTES`] of real content in between is
    /// NOT coalesced — a genuinely distinguishable command's output between
    /// two resizes must not have its resize markers silently merged.
    #[test]
    fn write_resize_marker_does_not_coalesce_across_substantial_content() {
        let mut rb = ScrollbackRingBuffer::new(1024 * 1024);
        rb.write_resize_marker(80, 24);
        rb.write(&vec![b'a'; (RESIZE_MARKER_COALESCE_MAX_BYTES + 1) as usize]);
        rb.write_resize_marker(81, 25);
        assert_eq!(
            rb.dim_markers_len(),
            2,
            "markers separated by substantial content must NOT coalesce"
        );
    }

    /// AC-6 shape (drag-resize): many dimension changes, each followed by a
    /// small redraw-sized chunk (well under the coalesce threshold) — the
    /// SAME pattern a real SIGWINCH storm produces (a TUI redraws its frame
    /// at every intermediate size) — must still collapse to very few
    /// `dim_markers` entries, not one per change.
    #[test]
    fn write_resize_marker_bounds_dim_markers_across_a_drag_resize_shaped_sequence() {
        let mut rb = ScrollbackRingBuffer::new(1024 * 1024);
        for step in 0..200u16 {
            rb.write_resize_marker(80 + step, 24 + (step % 5));
            rb.write(b"\x1b[2J\x1b[Hredraw frame");
        }
        assert!(
            rb.dim_markers_len() <= 2,
            "a drag-resize-shaped sequence (many changes, small redraws \
             between them) must collapse to a handful of dim_markers \
             entries, got {}",
            rb.dim_markers_len()
        );
    }

    /// AC-6 end-to-end: a drag-resize-shaped RECORDING (coalesced via
    /// `write_resize_marker`'s D4 logic), replayed through `TerminalCore`,
    /// costs at most a couple of reflows total — not one per intermediate
    /// resize — because the ring already collapsed the redundant markers
    /// before they ever reached the replay stream. (`TerminalCore`'s OWN
    /// replay-side coalescing, exercised by
    /// `term_core::terminal_core::tests::replay_coalesces_consecutive_markers_into_a_single_reflow`,
    /// only collapses markers with ZERO bytes between them — a real
    /// drag-resize always has redraw bytes between steps, so replay-side
    /// coalescing alone does not bound this; the recording-side coalescing
    /// this test exercises is what actually closes finding
    /// `5d1a4e9509365517`.)
    #[test]
    fn drag_resize_shaped_recording_replays_with_bounded_reflow_count() {
        let mut rb = ScrollbackRingBuffer::new(1024 * 1024);
        rb.write(b"before drag\r\n");
        for step in 0..200u16 {
            rb.write_resize_marker(80 + step, 24 + (step % 5));
            rb.write(b"\x1b[2J\x1b[Hredraw frame\r\n");
        }
        rb.write(b"after drag\r\n");
        let recording = rb.read_all();

        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 10_000);
        let before = core.reflow_call_count();
        core.reset_and_replay(&recording);
        let reflows = core.reflow_call_count() - before;
        assert!(
            reflows <= 2,
            "a drag-resize-shaped recording must replay with a bounded \
             reflow count (coalesced at the recording side), got {reflows} \
             reflows for 200 intermediate resizes"
        );
    }

    // ── task0003 AC-7 (D4, review round-2 finding `0b0c18ff4ab911f4`):
    // pruning is linear (VecDeque::pop_front), not quadratic ────────────

    /// A large number of markers, each separated by MORE than the coalesce
    /// threshold (so none collapse and `dim_markers` genuinely grows large)
    /// and all still within the CURRENT retained window (so none are
    /// prunable yet), followed by enough additional content to advance the
    /// retained window past ALL of them at once — forcing a single
    /// `prune_dim_markers` call to drop nearly the entire backlog in one
    /// pass. Asserts the STRUCTURAL outcome (bounded final length), which a
    /// correct prune must reach regardless of whether the underlying pop is
    /// O(1) (`VecDeque`, this fix) or O(n) (the previous `Vec::remove(0)`,
    /// quadratic overall for a pass dropping this many entries) — the
    /// complexity class itself is what `VecDeque::pop_front` fixes, not
    /// observable via a plain assertion, but this proves the fix doesn't
    /// change the OBSERVABLE result: it must still collapse to a bounded
    /// tail, not leak entries or leave the ring in an inconsistent state.
    #[test]
    fn prune_dim_markers_collapses_a_large_backlog_to_a_bounded_tail_in_one_pass() {
        let chunk_len = (RESIZE_MARKER_COALESCE_MAX_BYTES + 1) as usize;
        let marker_count = 2_000u32;
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
