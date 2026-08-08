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
    /// Cumulative count of `dim_markers` entries [`Self::enforce_dim_marker_cap`]
    /// has EVER popped for THIS ring (D1''''', round-8 rework, review
    /// round-7 finding `01f91fe698ceb287`; supersedes the round-6
    /// `dim_markers_evicted_by_cap: bool`, which could not distinguish
    /// "exactly one entry ever evicted" from "many").
    ///
    /// [`Self::prune_dim_markers`] maintains the invariant "if a qualifying
    /// head entry exists, keep at least it" on its own, so a `read_segments`
    /// call finding no entry at or before the retained window's head is
    /// otherwise a NORMAL, legitimate state (content genuinely predates the
    /// first-ever recorded resize — plain unattributed bytes, no segment).
    /// Only once the cap has ACTUALLY dropped entries can that same "no
    /// qualifying head" state instead mean "a predecessor that WOULD have
    /// qualified was forgotten". [`Self::read_segments`] uses the exact
    /// count, not just whether it is non-zero: reviewer measurement (round-7
    /// finding `01f91fe698ceb287`) showed [`Self::capped_head_dims`]'s
    /// single-entry fallback recovers EXACTLY the full-attribution result
    /// when precisely one entry was ever evicted, but attributing the WHOLE
    /// multi-entry gap to only the LAST of several evicted entries'
    /// dimensions can replay MORE cross-line mixing than not attributing the
    /// gap at all (e.g. 13 mixed rows vs 3 for "no segments" on one measured
    /// shape). Reset by [`Self::clear`].
    dim_markers_cap_eviction_count: u64,
    /// Dimensions of the MOST RECENTLY evicted `dim_markers` entry (D1'''',
    /// round-7 rework, review round-6 finding `bb3353636b0206cb`).
    ///
    /// [`Self::enforce_dim_marker_cap`] pops entries oldest-first; the LAST
    /// one it pops on any given call is therefore the one describing the
    /// span immediately preceding the oldest entry that survives — exactly
    /// the dimensions [`Self::read_segments`] needs for position 0 when
    /// EXACTLY ONE entry has ever been evicted (D1''''', round-8 rework):
    /// with two or more evictions this no longer describes the WHOLE gap
    /// (only its most recent slice), so `read_segments` stops using it as a
    /// head fallback in that case — see [`Self::dim_markers_cap_eviction_count`].
    /// Round-6 instead reused the oldest SURVIVING marker's dimensions for
    /// the gap (`mid[0]`, spliced to position 0) — those are the dimensions
    /// of the span that starts AFTER the gap, not the ones that produced it,
    /// so round-6 replayed the entire evicted-and-then-some span under an
    /// unrelated later resize's dimensions. Measured by the round-6
    /// reviewer: an apt-style phase followed by 26 resize markers (28 total,
    /// cap 24) replayed 7 mixed rows under that fallback, vs 3 for no
    /// attribution at all and 0 for full uncapped attribution — the round-6
    /// fallback was worse than shipping nothing. Reset by [`Self::clear`].
    capped_head_dims: Option<(u16, u16)>,
}

/// Hard ceiling on `dim_markers`' length, independent of redraw byte volume
/// (task0005 rework D3'', review round-4 finding `6c650908ea8e95e9`,
/// resolving the residual round-3 explicitly deferred as
/// `981230284d7d3273`).
///
/// D1'''''' (round-9 rework, review round-8 finding `6082de4e619d7f51`):
/// raised from 24 to 62. Every eviction — regardless of this constant's
/// value — is either 0/1 entries (exact match with full uncapped
/// attribution, see [`Self::read_segments`]'s doc) or 2+ entries (a
/// bounded-but-nonzero divergence that persists no matter how large this
/// cap is: this implementer re-measured it directly against the real
/// eviction mechanism and confirmed forcing 2+ evictions still mixes rows
/// at cap 62, exactly as it did at cap 24 — raising the cap does not make
/// that fallback smarter). What raising it changes is HOW LONG a resize
/// storm can run before it needs ANY eviction at all: at cap 24, a storm
/// recording more than 24 distinct dimensions already risked the 2+-
/// eviction case; at cap 62, a storm needs more than 62 distinct
/// dimensions to reach it. The round-8 reviewer's cap sweep (three fixed-
/// length storms, 26/32/52 total recorded markers, against caps 24/32/48/
/// 62) measured mixed rows 3/3/3 → 0/0/3 → 0/0/3 → **0/0/0**: each zero is
/// a storm short enough that the cap in question needed no eviction at
/// all, not a smarter fallback. 62 is the LARGEST cap that still keeps
/// this trivially-lossless regime, because it is the wire budget's
/// ceiling: `crates/mux_ipc::protocol::MAX_SEGMENTS` (64) minus one slot
/// for a synthesized head segment (single-eviction case) and one for a
/// trailing alt-screen dump segment (`build_snapshot_bytes_with_layout`'s
/// D7'' segment) — see `mux::session::pane::MAX_DAEMON_SNAPSHOT_SEGMENTS`.
/// AC-1's "resize storm of any length up to the wire ceiling" is this
/// exact regime: storms recording at most 62 distinct dimensions never
/// need the 2+-eviction fallback, so they always match full attribution;
/// a storm longer than that still only degrades to "never worse than no
/// attribution at all" (the pre-existing D2''''' hard gate), which this
/// change does not weaken.
///
/// This ceiling no longer needs to trade an ORDINARY switch's latency for
/// resize-storm headroom the way it did pre-round-6:
/// `TerminalCore::build_from_snapshot_inner`'s prefix/suffix split keeps
/// the (typically large) trailing run of segments that already match the
/// caller's target on the fast bypass path regardless of how many EARLIER
/// segments this ring recorded, so an ordinary switch's cost no longer
/// depends on this constant at all (round-8 measured 1.57 ms for an
/// ordinary switch against a segment-free 8.23 ms, both independent of the
/// cap) — raising it only affects the resize-STORM path (which the split
/// does not speed up — see that function's doc), where AC-1's requirement
/// is zero cross-line mixing, not latency. See `VERIFICATION.md` for the
/// storm-path latency measured at this new cap.
///
/// D1 (round-10 rework, task0010, D2): round-9 raised this to 62 purely
/// on wire-budget grounds because the STORM path's latency was, at the
/// time, a genuine trade-off against that raise — `TerminalCore::resize`
/// re-wrapped the entire scrollback accumulated so far on every
/// intermediate resize, so a longer-lived cap meant more segments meant
/// seconds of replay (see `crates/term_core/src/bench.rs`'s
/// `segment_bounded_replay_bench_950kib_stays_bounded_at_the_daemon_cap`'s
/// pre-round-10 doc: 24 segs → 323 ms / 62 → 4.5 s). Round-10's
/// `TerminalCore::resize_same_width` no longer re-wraps retained
/// scrollback for a same-width resize — a SAME-WIDTH resize storm (only
/// `rows` differs between markers; `cols` constant) is what every storm
/// fixture in this codebase's own tests actually drives this cap against
/// (this ring's own cap-sweep test, `pty_spawn.rs`'s
/// `run_resize_storm_cap_eviction_case`, and the bench above all hold
/// `cols` fixed) — so for that shape, the storm-path latency trade-off
/// this comment used to describe no longer applies to this constant's
/// VALUE: raising it further would still cost only the row-count-bounded
/// work D1 introduces, not work proportional to how many entries the cap
/// allows. A storm that ALSO varies `cols` between markers still pays the
/// pre-existing full-reflow cost per intermediate resize (D1 did not
/// attempt that harder, more general case — see `VERIFICATION.md`'s NFR1
/// section for the explicit scope statement). **62 remains the correct
/// value regardless**, because the wire budget (`MAX_SEGMENTS` minus the
/// head/screen slots, described above) is a separate, harder ceiling that
/// a flat same-width resize cost does not relax — this note exists so a
/// future reader does not see the wire-budget derivation above and wonder
/// whether a NOW-STALE latency trade-off still competes with it for the
/// SAME-WIDTH case. See `VERIFICATION.md`'s NFR1 section for the
/// re-measured storm-path latency at this cap after round-10.
///
/// Enforced by [`Self::enforce_dim_marker_cap`], called after every
/// [`Self::write_resize_marker`] append: when exceeded, the OLDEST entry is
/// simply dropped (see that method's doc for why round-6 stopped pulling
/// the successor's offset back to cover the dropped span) — every entry
/// more recent than the dropped one keeps its EXACT recorded attribution;
/// [`Self::read_segments`] loses precision only in the single oldest
/// surviving span, never a more recent one, and (D1'''', round-7 rework)
/// attributes that lost span to the LAST evicted entry's own dimensions
/// via `capped_head_dims` rather than to an unrelated later survivor's.
pub const MAX_DIM_MARKERS: usize = 62;

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
            dim_markers_cap_eviction_count: 0,
            capped_head_dims: None,
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
    /// resize.
    ///
    /// D1''' (round-6 rework, review round-5 finding `986a3881b2b97a16`):
    /// when the cap is exceeded, the OLDEST entry is simply dropped — it no
    /// longer pulls its successor's `offset` back to cover the dropped
    /// span. The old "pull back" merge moved a boundary across bytes that
    /// were ALREADY recorded under the successor's own dimensions, and
    /// repeating it on every subsequent overflow chained the offset back
    /// toward the pane's very first marker over a long resize storm — the
    /// entire retained window ended up replayed at a single mid-storm
    /// dimension pair (the coordinate drift this whole marker mechanism
    /// exists to prevent, reintroduced by the fix meant to bound
    /// `dim_markers`'s growth).
    ///
    /// D1'''' (round-7 rework, review round-6 finding `bb3353636b0206cb`):
    /// [`Self::read_segments`] tolerates the resulting gap by falling back
    /// to [`Self::capped_head_dims`] — the dimensions of the entry THIS
    /// method most recently dropped — for position 0, when no surviving
    /// marker's offset is at or before the retained window's head. Round-6
    /// instead fell back to the oldest SURVIVING marker's dimensions, which
    /// describes the span AFTER the gap, not the one that produced it;
    /// measured by the round-6 reviewer as replaying MORE mixed rows than
    /// not attributing the gap at all. Precision is lost only in the single
    /// oldest span (between the retained window's start and the oldest
    /// surviving marker's own recorded offset) — never a more recent one,
    /// and no entry's own recorded attribution is ever rewritten.
    fn enforce_dim_marker_cap(&mut self) {
        while self.dim_markers.len() > MAX_DIM_MARKERS {
            let Some((_, cols, rows)) = self.dim_markers.pop_front() else {
                break;
            };
            self.dim_markers_cap_eviction_count += 1;
            self.capped_head_dims = Some((cols, rows));
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
        // D1''''' (round-8 rework, review round-7 finding `01f91fe698ceb287`):
        // `enforce_dim_marker_cap` no longer pulls a surviving entry's
        // offset back to cover a dropped one, so after a cap-triggered drop
        // EVERY surviving marker's offset can be past `oldest_offset`
        // (nothing above qualified as `head`). When EXACTLY ONE entry has
        // ever been evicted, falling back to `capped_head_dims` — the
        // dimensions of that one entry — for position 0 recovers full
        // uncapped attribution exactly (the reviewer's measurement:
        // 0 mixed rows, matching the never-capped oracle).
        //
        // With TWO OR MORE evictions, `capped_head_dims` only names the
        // LAST of them — the gap spans everything every evicted entry
        // described, and attributing the whole thing to just the last one's
        // dimensions can replay MORE cross-line mixing than not attributing
        // the gap at all (reviewer measurement: up to 13 mixed rows under
        // that fallback vs 3 for "no segments", against 0 for full
        // attribution, on one tested shape). Leaving `head` unattributed
        // instead — [`TerminalCore::replay_segments`] then replays the
        // leading gap (before the first surviving segment's own offset) at
        // the caller's target dimensions — is never WORSE than "no
        // segments" (round-8 finding `6082de4e619d7f51`'s 216-shape sweep:
        // 0/216 shapes came out worse; this is the D2''''' hard gate this
        // ring's own tests enforce). It is NOT the same as full uncapped
        // attribution, though: round-9 rework (D2'''''', finding
        // `6eeccc889cb15d87`) corrected a prior version of this comment
        // that claimed otherwise — re-measured directly (this
        // implementer's own reproduction, `pty_spawn::tests::
        // resize_storm_beyond_marker_cap_replays_no_worse_than_full_attribution`),
        // 2+ evictions mix MORE rows than full attribution regardless of
        // how large [`MAX_DIM_MARKERS`] is (still true at 62, exactly as it
        // was at 24) — raising the cap does not change this fallback's
        // OUTCOME for a storm that genuinely needs 2+ evictions; it only
        // raises how many distinct dimensions a storm can record before it
        // needs any eviction at all (see [`MAX_DIM_MARKERS`]'s own doc).
        // Sibling doc `pty_spawn.rs`'s cap-eviction test states this same
        // guarantee correctly; this comment previously did not. Every
        // surviving marker keeps its own true position regardless (the
        // `mid` vec below is unaffected).
        //
        // Round-6 instead spliced `mid[0]` (the OLDEST SURVIVING marker) in
        // at position 0, discarding its true (later) offset — measured by
        // the round-6 reviewer as MORE mixed rows than not attributing the
        // gap at all (7 vs 3, against 0 for full uncapped attribution).
        //
        // Gated on `dim_markers_cap_eviction_count` — WITHOUT a prior cap
        // eviction (count == 0), "no qualifying head" is the NORMAL,
        // legitimate case of content that genuinely predates the
        // first-ever recorded resize (`prune_dim_markers` alone always
        // keeps a qualifying entry when one exists), and must stay
        // unattributed rather than be misassigned to a later resize's dims.
        if head.is_none() && self.dim_markers_cap_eviction_count == 1 {
            if let Some((cols, rows)) = self.capped_head_dims {
                head = Some((cols, rows));
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
        self.dim_markers_cap_eviction_count = 0;
        self.capped_head_dims = None;
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

    /// Capture this ring's full retained content for a handoff snapshot
    /// (task0003 AC-4). Round-trips with [`Self::load_snapshot`]: loading
    /// the returned snapshot reproduces [`Self::read_all`]'s bytes exactly.
    ///
    /// `dim_markers` history is deliberately NOT part of the captured
    /// snapshot — IMPLEMENTATION.md's Shared Components table pins the
    /// handoff document's per-pane fields, and scrollback is carried as
    /// plain bytes there, not structural segments (task0003 D8: terminal
    /// state is rebuilt by replaying restored scrollback, not by restoring
    /// byte-exact parser/segment state).
    pub fn capture(&self) -> ScrollbackSnapshot {
        ScrollbackSnapshot {
            capacity: self.capacity,
            data: self.read_all(),
        }
    }

    /// Reconstruct a ring from a captured snapshot (task0003 AC-4): writes
    /// `snapshot.data` into a fresh ring of `snapshot.capacity`, reproducing
    /// [`Self::read_all`]'s bytes byte-for-byte. Safe without truncation
    /// because `snapshot.data` is always the result of a prior
    /// [`Self::read_all`] call, which never exceeds its ring's capacity —
    /// as long as `snapshot.capacity` matches the capacity `data` was
    /// captured from (the normal case: [`Self::capture`] always pairs the
    /// two), the write below never wraps past what `data` already
    /// contains.
    pub fn load_snapshot(snapshot: &ScrollbackSnapshot) -> Self {
        let mut rb = Self::new(snapshot.capacity);
        rb.write(&snapshot.data);
        rb
    }
}

/// Captured whole-buffer scrollback state for a handoff snapshot (task0003
/// AC-4): the ring's full retained content, oldest-to-newest (identical to
/// what [`ScrollbackRingBuffer::read_all`] returns), plus the capacity
/// needed to reconstruct an equivalent ring. Round-trips with
/// [`ScrollbackRingBuffer::load_snapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbackSnapshot {
    pub capacity: usize,
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests;
