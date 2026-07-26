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
/// Rides the same envelope as the other `emterm` OSC 777 extensions (fold /
/// status-bar / agent-status / viewer launches — see
/// `crate::mux::scrollback_filter`), so it is:
/// - preserved byte-for-byte by `strip_replayable_rich_content`: that
///   function only strips viewer-launch kinds and `agent-status`; a `resize`
///   kind falls through to "kept" with no code change needed (see the
///   drift-guard test `scrollback_filter::strip_keeps_osc777_resize_marker`).
/// - inert to a marker-UNAWARE replay consumer: an unrecognized,
///   BEL-terminated OSC is parsed structurally and dropped without producing
///   a visible cell, both by an older `term_core` and by the daemon-side
///   shadow `vt100::Parser` (which never even sees this marker — it is
///   written directly into the pane's `scrollback` ring by `MuxPane::resize`,
///   not fed through the live PTY reader path).
///
/// `term_core` has no dependency on this (`emterm` mux) crate, so the byte
/// format itself — not a shared Rust type — is the contract between this
/// encoder and `term_core`'s decoder.
pub fn resize_marker_bytes(cols: u16, rows: u16) -> Vec<u8> {
    format!("\x1b]777;emterm;resize;{cols};{rows}\x07").into_bytes()
}

/// Circular byte buffer with fixed capacity.
///
/// Tracks resize-marker provenance (`dim_markers`) alongside the raw bytes
/// so [`Self::read_all`] can reconstruct a correct "dimensions in effect at
/// the oldest retained byte" marker even after ring wraparound evicts the
/// ORIGINAL marker bytes, in whole or in part (review round-1 rework,
/// findings `81947e02402b5ace` / `ee93d8be8823e5d7`, high). See
/// [`Self::write_resize_marker`] and [`Self::read_all`] for the mechanism.
pub struct ScrollbackRingBuffer {
    buf: Vec<u8>,
    capacity: usize,
    write_pos: usize,
    len: usize,
    /// Cumulative count of bytes ever passed to [`Self::write`] (including
    /// [`Self::write_resize_marker`]'s marker bytes), regardless of
    /// eviction. Monotonically increasing; used to compute the absolute
    /// stream offset of the oldest byte currently retained
    /// (`total_written.saturating_sub(capacity)`).
    total_written: u64,
    /// `(offset, cols, rows)` for every resize marker recorded via
    /// [`Self::write_resize_marker`] whose dimensions might still be needed
    /// to describe the current retained window's head — see that method's
    /// pruning and [`Self::read_all`]'s reconstruction. `offset` is the
    /// absolute stream position (`total_written` at the moment of the call)
    /// where the marker's bytes BEGIN. Always sorted ascending by `offset`
    /// (markers are appended in increasing offset order).
    dim_markers: Vec<(u64, u16, u16)>,
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
            dim_markers: Vec::new(),
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

    /// Write a resize marker (review round-1 rework, findings
    /// `81947e02402b5ace` / `ee93d8be8823e5d7`): like [`Self::write`] with
    /// `&resize_marker_bytes(cols, rows)`, but ALSO records where in the
    /// logical byte stream the marker begins, so [`Self::read_all`] can
    /// reconstruct it later even if ring wraparound evicts these exact
    /// bytes. `MuxPane::new` / `MuxPane::resize` call this instead of a
    /// plain `write` for every marker they record.
    pub fn write_resize_marker(&mut self, cols: u16, rows: u16) {
        let offset = self.total_written;
        self.dim_markers.push((offset, cols, rows));
        self.write(&resize_marker_bytes(cols, rows));
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
    /// fall back on.
    fn prune_dim_markers(&mut self) {
        let oldest_offset = self.total_written.saturating_sub(self.capacity as u64);
        while self.dim_markers.len() >= 2 && self.dim_markers[1].0 <= oldest_offset {
            self.dim_markers.remove(0);
        }
    }

    /// Read all accumulated data in chronological order.
    /// Returns a Vec containing the buffer contents from oldest to newest.
    ///
    /// When [`Self::write_resize_marker`] has ever been called on this ring,
    /// the returned bytes are reconstructed so the invariant "the retained
    /// window is preceded by a marker describing the dimensions in effect at
    /// its first byte" holds regardless of ring wraparound:
    ///
    /// - If the marker that established the current head's dimensions is
    ///   still fully intact at the front of the raw ring content (the common
    ///   case — nothing has evicted it yet), the raw bytes are returned
    ///   unchanged; they already start with that exact marker.
    /// - If ring wraparound evicted that marker in whole or in part, a
    ///   freshly-encoded marker for the SAME dimensions is prepended, and any
    ///   truncated remnant of the original marker's bytes still sitting at
    ///   the ring's front (which would otherwise surface as garbage visible
    ///   text — review round-1 rework AC-3) is dropped.
    ///
    /// A ring that never received a [`Self::write_resize_marker`] call (e.g.
    /// a bare `ScrollbackRingBuffer` used directly, outside `MuxPane`)
    /// behaves exactly as before — no synthetic bytes are ever introduced.
    pub fn read_all(&self) -> Vec<u8> {
        let raw = self.read_all_raw();
        let Some(&(marker_offset, cols, rows)) =
            self.dim_markers.iter().rev().find(|(offset, _, _)| {
                *offset <= self.total_written.saturating_sub(self.capacity as u64)
            })
        else {
            return raw;
        };
        let oldest_offset = self.total_written.saturating_sub(self.capacity as u64);
        if oldest_offset <= marker_offset {
            // The marker (if any of its bytes are even within the retained
            // window) is fully intact at the front of `raw` already.
            return raw;
        }
        let marker_bytes = resize_marker_bytes(cols, rows);
        let marker_end = marker_offset + marker_bytes.len() as u64;
        let skip = if oldest_offset < marker_end {
            (marker_end - oldest_offset) as usize
        } else {
            0
        };
        let skip = skip.min(raw.len());
        let mut out = Vec::with_capacity(marker_bytes.len() + raw.len() - skip);
        out.extend_from_slice(&marker_bytes);
        out.extend_from_slice(&raw[skip..]);
        out
    }

    /// The raw ring contents (no marker reconstruction) — the pre-task0002
    /// implementation of `read_all`, kept as the shared core both the
    /// no-markers-ever-written fast path and [`Self::read_all`]'s
    /// reconstruction build on.
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

    // ── review round-1 rework, findings 81947e02402b5ace / ee93d8be8823e5d7
    // (high) — task0002 AC-2 / AC-3: ring wraparound never loses the
    // "retained window is preceded by a marker" invariant ──────────────────

    /// `write_resize_marker` behaves like `write(&resize_marker_bytes(..))`
    /// content-wise when the ring never wraps: `read_all` returns the exact
    /// same bytes a plain `write` would have produced (no duplicate /
    /// synthetic marker prepended on top of the still-intact real one).
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

    /// AC-2: driving the ring past wraparound so the INITIAL marker is
    /// evicted still yields a `read_all()` that starts with a marker
    /// describing the dimensions in effect for whatever content survived —
    /// the last marker written before the retained window began.
    #[test]
    fn read_all_reconstructs_head_marker_after_initial_marker_evicted() {
        let capacity = 32;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(80, 24); // evicted once the ring wraps far enough
        // Push enough plain bytes to wrap the ring several times over,
        // evicting the initial marker entirely.
        rb.write(&vec![b'x'; capacity * 3]);
        let out = rb.read_all();
        // The raw ring content is entirely `x` bytes at this point (the
        // marker was long since overwritten); `read_all` prepends a
        // synthetic marker on top of that raw content, so the assembled
        // output is `capacity` (marker bytes) LONGER than `capacity`.
        let expected = [resize_marker_bytes(80, 24), vec![b'x'; capacity]].concat();
        assert_eq!(
            out, expected,
            "retained window must be preceded by a marker for the dims that \
             produced it, even though the original marker bytes were evicted"
        );
    }

    /// AC-2 (multi-resize variant): after TWO resizes and enough content to
    /// evict both the initial marker and the FIRST resize's marker, the
    /// reconstructed head marker reflects the SECOND (most recent
    /// surviving) resize's dimensions, not the first.
    #[test]
    fn read_all_reconstructs_head_marker_using_latest_surviving_resize() {
        let capacity = 64;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(80, 24);
        rb.write(b"some content produced at the first size");
        rb.write_resize_marker(120, 40);
        // Push enough plain bytes to evict everything before this point.
        rb.write(&vec![b'y'; capacity * 3]);
        let out = rb.read_all();
        assert!(
            out.starts_with(&resize_marker_bytes(120, 40)),
            "head marker must reflect the most recent resize whose region \
             still precedes the retained window: {out:?}"
        );
        let first_marker = resize_marker_bytes(80, 24);
        assert!(
            !out.windows(first_marker.len()).any(|w| w == first_marker),
            "no trace of the evicted first marker should remain: {out:?}"
        );
        assert!(
            !out.windows(b"first size".len()).any(|w| w == b"first size"),
            "no trace of the evicted first-resize content should remain: {out:?}"
        );
    }

    /// AC-3: when ring eviction cuts a resize marker's byte sequence in
    /// half (the introducer is gone but a tail remnant of its digits/BEL
    /// survives at the ring's front), `read_all()` must drop that remnant —
    /// not let it surface as visible garbage text — while still prepending
    /// a clean synthetic marker for the correct dimensions.
    #[test]
    fn read_all_drops_truncated_marker_remnant_never_renders_as_garbage() {
        let marker = resize_marker_bytes(65500, 65500); // fixed-width digits, easy to reason about
        let marker_len = marker.len();
        // Capacity chosen so eviction lands exactly mid-marker: after the
        // marker plus a short content run, we push exactly enough extra
        // bytes that the ring's oldest retained byte falls partway through
        // the marker itself.
        let capacity = marker_len + 4;
        let mut rb = ScrollbackRingBuffer::new(capacity);
        rb.write_resize_marker(65500, 65500);
        // Overwrite everything except the marker's LAST 2 bytes plus 4
        // fresh bytes, so eviction truncates the marker down to a 2-byte
        // remnant sitting at the ring's front pre-reconstruction.
        let evict_amount = marker_len - 2;
        rb.write(&vec![b'z'; evict_amount + 4]);

        let out = rb.read_all();
        // The raw ring content (pre-reconstruction) is a 2-byte truncated
        // tail of the marker followed by all `evict_amount + 4` fresh `z`
        // bytes; `read_all` drops that 2-byte remnant and prepends a clean
        // synthetic marker, so the assembled output is exactly the marker
        // followed by the full `z` run (no length lost, no remnant left).
        let expected = [marker.clone(), vec![b'z'; evict_amount + 4]].concat();
        assert_eq!(
            out, expected,
            "a clean synthetic marker must replace the truncated remnant, \
             and only fresh content bytes must follow it"
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
}
